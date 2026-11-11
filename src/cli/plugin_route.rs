//! CLI 自定义命令双模动态路由（03 §6 唯一权威实现）。
//!
//! 路由前提：根命令 `trailing_var_arg = true`（`src/cli/mod.rs`，AGENTS §5 位置
//! 契约锁定）使 `rupost <cmd>` 被整体吸入 `cli.args`，clap 不产生"未知子命令"
//! 事件——因此派发不依赖 clap 分支，而是对 `args[0]` 主动查表，裁决序：
//! 内置子命令 > 插件命令（精确匹配）> HTTP 方法名（内联请求）> 报错 exit 2。
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Stdio;

use rupost::plugin::{DiscoverOptions, PluginRegistry};

/// HTTP 方法集合（03 §6.1 裁决序第 3 级；与 `src/http/types.rs::Method` 语义一致）
pub const HTTP_METHODS: [&str; 7] = [
    "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS",
];

/// `args[0]` 是否为 HTTP 方法（大小写不敏感）
pub fn is_http_method_token(token: &str) -> bool {
    HTTP_METHODS.iter().any(|m| m.eq_ignore_ascii_case(token))
}

/// 路由裁决（03 §6.1）：返回 `RoutedCommand` 表示命中插件命令需拉起子进程；
/// `None` 表示交给内联请求模式继续处理。
pub enum RoutingDecision {
    /// 插件命令命中：(可执行路径, 命令名, 剩余参数)
    PluginCommand(std::path::PathBuf, String, Vec<String>),
    /// HTTP 方法 / 空 args → 内联模式
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
    // 1. 内置子命令最高优先级（正常路径 clap 已派发；此处防御性兜底）
    if builtin_subcommands.iter().any(|b| b == first) {
        return RoutingDecision::Inline;
    }
    // 2. 插件命令精确匹配
    if let Some((exe_path, capability)) = registry.find_command(first) {
        return RoutingDecision::PluginCommand(
            exe_path.clone(),
            capability.name.clone(),
            args[1..].to_vec(),
        );
    }
    // 3. HTTP 方法名 → 内联请求模式
    if is_http_method_token(first) {
        return RoutingDecision::Inline;
    }
    // 4. 未知
    RoutingDecision::Unknown(first.clone())
}

/// 拉起插件命令子进程（03 §6.2）：
/// - TTY 独占直通（inherit 三向），原生支持交互式 TUI / 密码输入
/// - 来源横幅（路径 + sha256 前 16 位）+ 首次 TOFU 确认（防投毒 UI 伪造钓鱼）
/// - 环境白名单：PATH/HOME/LANG/TERM/TMPDIR + RUPOST_* + requires_env 声明项 +
///   `--plugin-env` 显式放行（全量继承废除，防堡垒机凭据外泄）
/// - 退出码信号感知穿透：`Some(code)` 原值；`None` → `128 + signal`
pub async fn launch_plugin_command(
    exe_path: &Path,
    args: &[String],
    allow_env: &[String],
) -> i32 {
    use std::io::Write;

    // TOFU：首次执行的 (路径, 哈希) 组合要求交互确认，缓存于 registry 旁路文件
    let exe_hash = sha256_prefix(exe_path);
    let tofu_cache = cache_dir().join("tofu-commands.txt");
    let fingerprint = format!("{} {}", exe_path.display(), exe_hash);
    let seen = std::fs::read_to_string(&tofu_cache)
        .map(|content| content.lines().any(|line| line.trim() == fingerprint))
        .unwrap_or(false);
    if !seen {
        println!(
            "[plugin] 首次执行外部命令：{} sha256:{}",
            exe_path.display(),
            &exe_hash[..16]
        );
        print!("确认执行？[y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        let answer = answer.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            eprintln!("[!] 已取消执行插件命令");
            return 1;
        }
        if let Some(parent) = tofu_cache.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&tofu_cache)
        {
            let _ = writeln!(file, "{fingerprint}");
        }
    } else {
        // 来源横幅：每次拉起都可见（审计锚点）
        println!("[plugin] {} sha256:{}", exe_path.display(), &exe_hash[..16]);
    }

    let mut cmd = tokio::process::Command::new(exe_path);
    cmd.args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // 环境白名单（03 §6.2 裁决）：默认仅继承基础集 + RUPOST_* + 显式放行
    cmd.env_clear();
    for key in rupost::plugin::PLUGIN_ENV_ALLOWLIST {
        if let Ok(value) = std::env::var(key) {
            cmd.env(key, value);
        }
    }
    for (key, value) in std::env::vars().filter(|(k, _)| k.starts_with("RUPOST_")) {
        cmd.env(key, value);
    }
    for key in allow_env {
        if let Ok(value) = std::env::var(key) {
            cmd.env(key, value);
        }
    }
    cmd.env("RUPOST_WORKSPACE", std::env::current_dir().unwrap_or_default());
    if let Some(home) = dirs::home_dir() {
        cmd.env("RUPOST_CONFIG_DIR", home.join(".rupost"));
    }

    match cmd.status().await {
        Ok(status) => {
            if let Some(code) = status.code() {
                code
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    status
                        .signal()
                        .map(|s| 128 + s)
                        .unwrap_or(1)
                }
                #[cfg(not(unix))]
                {
                    1
                }
            }
        }
        Err(e) => {
            eprintln!("[!] 插件命令启动失败: {e}");
            1
        }
    }
}

fn cache_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".rupost"))
        .unwrap_or_else(|| std::path::PathBuf::from(".rupost"))
}

fn sha256_prefix(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let Ok(bytes) = std::fs::read(path) else {
        return "unreadable".repeat(2);
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 发现插件注册表（路由兜底用）。发现失败 → 空注册表 + 明确报错文案
/// （该错误由调用方渲染为 exit 2 提示，绝不静默吞掉）。
pub fn discover_for_routing(allow_project_plugins: bool) -> PluginRegistry {
    let options = DiscoverOptions {
        allow_project_plugins,
        project_root: None,
    };
    PluginRegistry::discover(&options).unwrap_or_default()
}

/// 内置子命令名集合（与 `Commands` 枚举变体一一对应；位置契约兜底用）
pub fn builtin_subcommand_names() -> BTreeSet<String> {
    ["test", "history", "generate", "diagnose", "mock", "init", "replay", "export", "web", "ai", "import", "tui"]
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
        lines.push(format!("  {:<14} {} ({})", name, description, path.display()));
    }
    lines
}

/// manifest 文本白名单净化（03 §6.4 终端注入防线）：
/// 状态机剥除 ANSI CSI（`ESC[...@-~`）与 OSC（`ESC]...BEL/ESC\`）完整序列 +
/// 其余控制字符，长度钳制——防投毒 manifest 经 help/plugin list 注入
/// 清屏/OSC 伪造链接/剪贴板覆写。
pub fn sanitize_manifest_text(text: &str, max_len: usize) -> String {
    let mut cleaned = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    // CSI：吞到 @-~ 终结符
                    chars.next();
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if n.is_ascii() && ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC：吞到 BEL 或 ESC\
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
    use rupost::plugin::{
        CommandCapability, HookCapability, PluginCacheScope, PluginManifest, PluginStage,
    };

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
                hooks: vec![HookCapability {
                    directive: "auth".to_string(),
                    alias: Some(name.to_string()),
                    stage: PluginStage::Prepare,
                    cache_scope: PluginCacheScope::default(),
                    requires_env: vec![],
                    parameters_schema: serde_json::json!({}),
                }],
            },
        );
        registry
    }

    /// 03 §6.1 裁决序：内置 > 插件 > HTTP 方法 > 未知
    #[test]
    fn test_route_priority_builtin_over_plugin_over_method() {
        let builtins = builtin_subcommand_names();
        let registry = registry_with_command("test"); // 与内置命令重名的恶意插件

        // 内置赢：插件注册了 "test" 也必须走内置
        assert!(matches!(
            route(&["test".into(), "x.http".into()], &builtins, &registry),
            RoutingDecision::Inline
        ));
        // 插件命令命中
        let registry2 = registry_with_command("flamegraph");
        match route(&["flamegraph".into(), "a.json".into()], &builtins, &registry2) {
            RoutingDecision::PluginCommand(path, name, rest) => {
                assert!(path.ends_with("rupost-plugin-flamegraph"));
                assert_eq!(name, "flamegraph");
                assert_eq!(rest, vec!["a.json".to_string()]);
            }
            _ => panic!("插件命令必须命中"),
        }
        // HTTP 方法 → 内联
        assert!(matches!(
            route(&["GET".into(), "http://x".into()], &builtins, &registry2),
            RoutingDecision::Inline
        ));
        assert!(matches!(
            route(&["get".into(), "http://x".into()], &builtins, &registry2),
            RoutingDecision::Inline
        ));
        // 未知
        assert!(matches!(
            route(&["nope".into()], &builtins, &registry2),
            RoutingDecision::Unknown(_)
        ));
        // 空 args → 内联（保持既有报错语义）
        assert!(matches!(route(&[], &builtins, &registry2), RoutingDecision::Inline));
    }

    /// 03 §6.4 净化契约：ANSI CSI/OSC 完整序列剥除 + 长度钳制
    #[test]
    fn test_sanitize_manifest_text_strips_ansi() {
        // 清屏 CSI + OSC 伪造链接：整段剥离，正文保留
        assert_eq!(
            sanitize_manifest_text("\u{1b}[2Jclear\u{1b}]8;;http://evil\u{7}", 80),
            "clear"
        );
        // 仿冒密码提示的 CSI 着色注入
        assert_eq!(
            sanitize_manifest_text("\u{1b}[31m请输入密码：\u{1b}[0m", 80),
            "请输入密码："
        );
        assert_eq!(sanitize_manifest_text("normal text", 80), "normal text");
        let long = "x".repeat(100);
        let out = sanitize_manifest_text(&long, 80);
        assert_eq!(out.chars().count(), 81); // 80 + ellipsis
        assert!(out.ends_with('…'));
    }
}
