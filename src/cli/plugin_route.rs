//! CLI 自定义命令双模动态路由与 `rupost plugin` 子命令处理（03 §6 唯一权威实现）。
//!
//! 裁决序：内置子命令 > 插件命令（精确匹配）> HTTP 方法/URL（内联请求）> 报错 exit 2。

use std::collections::BTreeSet;

use rupost::Result;
use rupost::http::Method;
use rupost::plugin::{DiscoverOptions, PluginRegistry};

/// 解析后的动态 CLI 路由参数（剥离 rupost 自有插件旗标，保留纯净业务参数传给插件）
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedRouteArgs {
    pub allow_project_plugins: bool,
    pub allow_env: Vec<String>,
    pub passthrough: Vec<String>,
}

/// 从 `cli.args` 任意位置提取 `--allow-project-plugins` 与 `--plugin-env[=...]`
pub fn parse_routing_args(args: &[String]) -> Result<ParsedRouteArgs> {
    let mut parsed = ParsedRouteArgs {
        allow_project_plugins: rupost::security::fs::env_flag_enabled(
            "RUPOST_ALLOW_PROJECT_PLUGINS",
        ),
        ..Default::default()
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--allow-project-plugins" {
            parsed.allow_project_plugins = true;
        } else if let Some(spec) = arg.strip_prefix("--plugin-env=") {
            if spec.is_empty() {
                return Err(rupost::RupostError::Other(
                    "--plugin-env= 缺少环境变量名".to_string(),
                ));
            }
            parsed.allow_env.push(spec.to_string());
        } else if arg == "--plugin-env" {
            let Some(spec) = iter.next() else {
                return Err(rupost::RupostError::Other(
                    "--plugin-env 缺少环境变量名参数（用法：--plugin-env KEY 或 --plugin-env KEY=VALUE）"
                        .to_string(),
                ));
            };
            if spec.is_empty() {
                return Err(rupost::RupostError::Other(
                    "--plugin-env 环境变量名不能为空".to_string(),
                ));
            }
            parsed.allow_env.push(spec.clone());
        } else {
            parsed.passthrough.push(arg.clone());
        }
    }
    Ok(parsed)
}

/// `args[0]` 是否为标准 HTTP 方法（复用 `rupost::http::Method` SSOT）
pub fn is_http_method_token(token: &str) -> bool {
    Method::is_standard(token)
}

/// `args[0]` 是否为 URL 直接内联形态（curl 风格省略方法）
pub fn is_url_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ws://")
        || lower.starts_with("wss://")
        || lower.starts_with("localhost")
        || lower.starts_with("127.")
        || lower.starts_with("0.0.0.0")
        || lower.starts_with("[::1]")
}

/// 路由裁决（03 §6.1）
pub enum RoutingDecision {
    /// 插件命令命中：(可执行路径, 命令名, 剩余参数)
    PluginCommand(std::path::PathBuf, String, Vec<String>),
    /// HTTP 方法 / URL / 空 args → 内联模式
    Inline,
    /// 未知命令（报错 exit 2）
    Unknown(String),
}

/// 对 `args[0]` 查表裁决。`builtin_subcommands` 来自 clap `Commands` 变体名。
pub fn route(
    args: &[String],
    builtin_subcommands: &BTreeSet<String>,
    registry: &PluginRegistry,
) -> RoutingDecision {
    let Some(first) = args.first() else {
        return RoutingDecision::Inline;
    };
    if builtin_subcommands.iter().any(|b| b == first) {
        return RoutingDecision::Inline;
    }
    if let Some((exe_path, capability)) = registry.find_command(first) {
        return RoutingDecision::PluginCommand(
            exe_path.clone(),
            capability.name.clone(),
            args[1..].to_vec(),
        );
    }
    if is_http_method_token(first) || is_url_token(first) {
        return RoutingDecision::Inline;
    }
    RoutingDecision::Unknown(first.clone())
}

fn registry_cache_path() -> std::path::PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".rupost").join("registry.json"))
        .unwrap_or_else(|| std::path::PathBuf::from(".rupost-registry.json"))
}

/// 发现插件注册表（路由兜底用）。优先命中磁盘缓存（<5ms），发现失败/冲突告警可见输出。
pub fn discover_for_routing(allow_project_plugins: bool) -> PluginRegistry {
    let options = DiscoverOptions {
        allow_project_plugins,
        project_root: None,
    };
    let cache_path = registry_cache_path();
    match PluginRegistry::load_or_discover(&options, &cache_path) {
        Ok(reg) => {
            for warning in &reg.warnings {
                eprintln!("[!] {warning}");
            }
            reg
        }
        Err(e) => {
            eprintln!("[!] 插件注册表发现失败: {e}");
            PluginRegistry::default()
        }
    }
}

/// 处理 `rupost plugin list|schema` 子命令
pub fn handle_plugin_command(
    command: crate::cli::PluginCommands,
    allow_project_plugins: bool,
) -> Result<()> {
    let options = DiscoverOptions {
        allow_project_plugins: allow_project_plugins
            || rupost::security::fs::env_flag_enabled("RUPOST_ALLOW_PROJECT_PLUGINS"),
        project_root: None,
    };
    let cache_path = registry_cache_path();
    let registry = PluginRegistry::load_or_discover(&options, &cache_path)?;
    for warning in &registry.warnings {
        eprintln!("[!] {warning}");
    }
    match command {
        crate::cli::PluginCommands::List => {
            println!("Installed plugin commands:");
            if registry.commands.is_empty() {
                println!(
                    "  (none) — 安装方式：将 rupost-plugin-<name> 二进制与同名 .json 静态 manifest 放入 $PATH、~/.rupost/plugins 或 ./.rupost/plugins（需 --allow-project-plugins）"
                );
            }
            for line in render_extensions(&registry) {
                println!("{line}");
            }
            println!("Lifecycle hooks:");
            let mut hook_names: Vec<_> = registry.hooks.keys().collect();
            hook_names.sort();
            if hook_names.is_empty() {
                println!("  (none)");
            }
            for name in hook_names {
                let (path, hook) = &registry.hooks[name];
                println!("  {:<14} stage={:?} @ {}", name, hook.stage, path.display());
            }
        }
        crate::cli::PluginCommands::Schema => {
            let mut defs = serde_json::Map::new();
            let mut aliases: Vec<_> = registry.hooks.keys().collect();
            aliases.sort();
            for alias in aliases {
                let (_, hook) = &registry.hooks[alias];
                defs.insert(
                    format!("@{} {}", hook.directive, alias),
                    hook.parameters_schema.clone(),
                );
            }
            let schema = serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "title": "rupost plugin directive parameters",
                "type": "object",
                "properties": defs,
            });
            println!("{}", serde_json::to_string_pretty(&schema)?);
        }
    }
    Ok(())
}

/// 内置子命令名集合（与 `Commands` 枚举变体一一对应）
pub fn builtin_subcommand_names() -> BTreeSet<String> {
    [
        "test", "history", "generate", "diagnose", "mock", "init", "replay", "export", "web", "ai",
        "import", "plugin", "tui",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// EXTENSIONS 区块渲染（03 §6.3）：manifest 文本字段白名单净化后聚合展示
pub fn render_extensions(registry: &PluginRegistry) -> Vec<String> {
    let mut lines = Vec::new();
    let mut names: Vec<_> = registry.commands.keys().collect();
    names.sort();
    for name in names {
        let (path, capability) = &registry.commands[name];
        let description = sanitize_manifest_text(&capability.description, 80);
        lines.push(format!(
            "  {:<14} {} ({})",
            name,
            description,
            path.display()
        ));
    }
    lines
}

/// manifest 文本白名单净化（03 §6.4 终端注入防线）：剥除 ANSI CSI/OSC 与控制字符并截断。
pub fn sanitize_manifest_text(text: &str, max_len: usize) -> String {
    let mut cleaned = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if n.is_ascii() && ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' {
                            break;
                        }
                        if n == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        if !c.is_control() {
            cleaned.push(c);
        }
    }
    let cleaned = cleaned.trim();
    if cleaned.chars().count() > max_len {
        let truncated: String = cleaned.chars().take(max_len).collect();
        format!("{truncated}…")
    } else {
        cleaned.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rupost::plugin::{CommandCapability, PluginManifest};

    fn registry_with_command(name: &str) -> PluginRegistry {
        let mut registry = PluginRegistry::new();
        registry.register_manifest(
            std::path::PathBuf::from(format!("/bin/rupost-plugin-{name}")),
            PluginManifest {
                manifest_version: "1.0".to_string(),
                name: name.to_string(),
                version: "1.0".to_string(),
                description: "demo".to_string(),
                commands: vec![CommandCapability {
                    name: name.to_string(),
                    usage: format!("rupost {name}"),
                    description: "demo".to_string(),
                    requires_tty: false,
                }],
                hooks: vec![],
            },
        );
        registry
    }

    #[test]
    fn test_route_priority_builtin_over_plugin_over_method() {
        let builtins = builtin_subcommand_names();
        assert!(builtins.contains("plugin"), "内置清单必须包含 plugin");
        let registry = registry_with_command("test");
        assert!(matches!(
            route(&["test".into(), "x.http".into()], &builtins, &registry),
            RoutingDecision::Inline
        ));

        let registry_plugin = registry_with_command("plugin");
        assert!(matches!(
            route(
                &["plugin".into(), "list".into()],
                &builtins,
                &registry_plugin
            ),
            RoutingDecision::Inline
        ));

        let registry2 = registry_with_command("flamegraph");
        match route(
            &["flamegraph".into(), "a.json".into()],
            &builtins,
            &registry2,
        ) {
            RoutingDecision::PluginCommand(path, name, rest) => {
                assert!(path.ends_with("rupost-plugin-flamegraph"));
                assert_eq!(name, "flamegraph");
                assert_eq!(rest, vec!["a.json".to_string()]);
            }
            _ => panic!("插件命令必须命中"),
        }
        assert!(matches!(
            route(&["GET".into(), "http://x".into()], &builtins, &registry2),
            RoutingDecision::Inline
        ));
        assert!(matches!(
            route(&["http://x/login".into()], &builtins, &registry2),
            RoutingDecision::Inline
        ));
        assert!(matches!(
            route(&["localhost:8080/health".into()], &builtins, &registry2),
            RoutingDecision::Inline
        ));
        assert!(matches!(
            route(&["nope".into()], &builtins, &registry2),
            RoutingDecision::Unknown(_)
        ));
        assert!(matches!(
            route(&[], &builtins, &registry2),
            RoutingDecision::Inline
        ));
    }

    #[test]
    fn test_sanitize_manifest_text_strips_ansi() {
        assert_eq!(
            sanitize_manifest_text("\u{1b}[2Jclear\u{1b}]8;;http://evil\u{7}", 80),
            "clear"
        );
        assert_eq!(
            sanitize_manifest_text("\u{1b}[31m请输入密码：\u{1b}[0m", 80),
            "请输入密码："
        );
        let out = sanitize_manifest_text(&"x".repeat(100), 80);
        assert_eq!(out.chars().count(), 81);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn test_parse_routing_args_dangling_plugin_env_errors_and_strips_flags() {
        let err = parse_routing_args(&["GET".into(), "http://x".into(), "--plugin-env".into()])
            .expect_err("缺少值的 --plugin-env 必须返回错误");
        assert!(err.to_string().contains("--plugin-env"));

        let parsed = parse_routing_args(&[
            "POST".into(),
            "http://x".into(),
            "--allow-project-plugins".into(),
            "--plugin-env=FOO=1".into(),
            "a=b".into(),
        ])
        .unwrap();
        assert!(parsed.allow_project_plugins);
        assert_eq!(parsed.allow_env, vec!["FOO=1".to_string()]);
        assert_eq!(
            parsed.passthrough,
            vec![
                "POST".to_string(),
                "http://x".to_string(),
                "a=b".to_string()
            ]
        );
    }
}
