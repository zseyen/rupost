//! CLI 根模块：`Cli` 结构体与全局标志、子命令重导出（解耦提案 v2 Phase 4）。
//! 子命令参数见 `commands/`，派发见 `dispatch.rs`，内联请求运行见 `runner.rs`。

mod commands;
mod dispatch;
mod parser;
mod plugin_route;
mod runner;

use clap::Parser;

// 路径稳定重导出：crate::cli::{Commands, XxxArgs} 与迁移前一致（bin crate 内
// 未被 mod.rs 直接消费的类型会触发 unused_imports，显式豁免）
#[allow(unused_imports)]
pub use commands::{
    AiCommands, Commands, DiagnoseArgs, ExportArgs, GenerateArgs, HistoryCommands, ImportCommands,
    ImportCurlArgs, InitArgs, MockArgs, ReplayArgs, TestArgs, TuiArgs, WebArgs,
};
pub use dispatch::dispatch;
#[allow(unused_imports)]
pub use runner::RunParams;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// 可选参数用于默认运行(curl/httpie 风格)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Disable automatic cookie handling for CLI requests
    #[arg(long, global = true)]
    pub no_cookies: bool,

    /// File path for persistent cookie storage for CLI requests
    #[arg(long, global = true, value_name = "FILE")]
    pub cookie_file: Option<String>,

    /// Enable detailed network diagnostics timing
    #[arg(long, global = true)]
    pub debug: bool,

    /// Enable detailed network diagnostics only on failure
    #[arg(long, global = true)]
    pub debug_on_failure: bool,

    /// Disable request history recording for this run (CI/共享环境防凭据落盘)
    #[arg(long, global = true)]
    pub no_history: bool,

    /// Default scheme when URL does not contain one
    #[arg(long, global = true, default_value = "http")]
    pub default_scheme: String,

    /// Environment name (e.g., dev, staging, prod) — resolves `--cookie-file`
    /// to an environment-suffixed file for inline requests, mirroring `test`
    #[arg(long, global = true, value_name = "NAME")]
    pub env: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_args_and_run_params_mapping() {
        let input = vec![
            "rupost",
            "--env",
            "prod",
            "--default-scheme",
            "http",
            "export",
            "curl",
            "tests/sample.http",
            "--compact",
            "--redact",
        ];
        let cli = Cli::try_parse_from(&input).expect("parse failed");
        assert_eq!(cli.env.as_deref(), Some("prod"));
        assert_eq!(cli.default_scheme, "http");

        match cli.command {
            Some(Commands::Export(args)) => {
                assert_eq!(args.target.as_deref(), Some("curl"));
                assert_eq!(args.paths, vec!["tests/sample.http"]);
                assert!(args.compact);
                assert!(args.redact);

                let run_opts = args.into_run_options(
                    cli.env.clone(),
                    cli.cookie_file.clone(),
                    cli.default_scheme.clone(),
                );
                assert_eq!(run_opts.target.as_deref(), Some("curl"));
                assert_eq!(run_opts.env.as_deref(), Some("prod"));
                assert_eq!(run_opts.default_scheme, "http");
                assert!(run_opts.compact);
                assert!(run_opts.redact);
            }
            _ => panic!("Expected Commands::Export"),
        }

        let inline_input = vec!["rupost", "--debug", "GET", "https://api.example.com"];
        let inline_cli = Cli::try_parse_from(&inline_input).expect("inline parse failed");
        let params = RunParams::from_cli(&inline_cli);
        assert_eq!(params.args, vec!["GET", "https://api.example.com"]);
        assert!(params.debug);
        assert!(!params.debug_on_failure);
    }

    #[test]
    fn test_run_params_builder_and_default() {
        // 1. 测试默认值规范
        let default_params = RunParams::default();
        assert!(default_params.args.is_empty());
        assert!(!default_params.no_cookies);
        assert_eq!(default_params.cookie_file, None);
        assert_eq!(default_params.env, None);
        assert_eq!(default_params.default_scheme, "http");
        assert!(!default_params.debug);
        assert!(!default_params.debug_on_failure);

        // 2. 测试 new 构造器与链式 Builder
        let custom_params = RunParams::new(["POST", "http://127.0.0.1:8080/api"])
            .with_env("staging")
            .with_default_scheme("https")
            .with_cookie_file("/tmp/cookies.json")
            .with_no_cookies(true)
            .with_debug(true)
            .with_debug_on_failure(true);

        assert_eq!(
            custom_params.args,
            vec!["POST", "http://127.0.0.1:8080/api"]
        );
        assert_eq!(custom_params.env.as_deref(), Some("staging"));
        assert_eq!(custom_params.default_scheme, "https");
        assert_eq!(
            custom_params.cookie_file.as_deref(),
            Some("/tmp/cookies.json")
        );
        assert!(custom_params.no_cookies);
        assert!(custom_params.debug);
        assert!(custom_params.debug_on_failure);

        // 3. 测试 ExportArgs 默认值
        let export_args = ExportArgs::default();
        assert_eq!(export_args.target, None);
        assert!(export_args.paths.is_empty());
        assert!(!export_args.compact);
        assert!(!export_args.redact);
    }

    /// `--no-history` 全局旗标位置契约：子命令前后双序均可解析（AGENTS §5 同源规则），
    /// 且不污染 trailing_var_arg 的内联 args
    #[test]
    fn test_no_history_position_contract() {
        let pre = Cli::try_parse_from(["rupost", "--no-history", "test", "suite.http"])
            .expect("pre-subcommand parse failed");
        assert!(pre.no_history);
        assert!(matches!(pre.command, Some(Commands::Test(_))));

        let post = Cli::try_parse_from(["rupost", "test", "suite.http", "--no-history"])
            .expect("post-subcommand parse failed");
        assert!(post.no_history);
        assert!(matches!(post.command, Some(Commands::Test(_))));

        let inline = Cli::try_parse_from(["rupost", "--no-history", "GET", "http://x"])
            .expect("inline parse failed");
        assert!(inline.no_history);
        assert_eq!(inline.args, vec!["GET", "http://x"]);

        let default_off =
            Cli::try_parse_from(["rupost", "test", "suite.http"]).expect("default parse failed");
        assert!(!default_off.no_history);
    }

    struct TestCase {
        name: &'static str,
        input: Vec<&'static str>,
        expected_debug: bool,
        expected_debug_on_failure: bool,
        expected_env: Option<&'static str>,
        expected_has_subcommand: bool,
        expected_args: Vec<&'static str>,
    }

    #[test]
    fn test_global_args_position_contract_table() {
        let cases = vec![
            TestCase {
                name: "pre_subcmd_debug",
                input: vec!["rupost", "--debug", "test", "suite.http"],
                expected_debug: true,
                expected_debug_on_failure: false,
                expected_env: None,
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "post_subcmd_debug",
                input: vec!["rupost", "test", "suite.http", "--debug"],
                expected_debug: true,
                expected_debug_on_failure: false,
                expected_env: None,
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "pre_subcmd_debug_on_failure",
                input: vec!["rupost", "--debug-on-failure", "test", "suite.http"],
                expected_debug: false,
                expected_debug_on_failure: true,
                expected_env: None,
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "post_subcmd_debug_on_failure",
                input: vec!["rupost", "test", "suite.http", "--debug-on-failure"],
                expected_debug: false,
                expected_debug_on_failure: true,
                expected_env: None,
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "mixed_flags_both_positions",
                input: vec![
                    "rupost",
                    "--debug",
                    "test",
                    "suite.http",
                    "--debug-on-failure",
                ],
                expected_debug: true,
                expected_debug_on_failure: true,
                expected_env: None,
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "global_value_arg_pre",
                input: vec!["rupost", "--env", "staging", "test", "suite.http"],
                expected_debug: false,
                expected_debug_on_failure: false,
                expected_env: Some("staging"),
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "global_value_arg_post",
                input: vec!["rupost", "test", "suite.http", "--env", "staging"],
                expected_debug: false,
                expected_debug_on_failure: false,
                expected_env: Some("staging"),
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "inline_mode_pre_debug",
                input: vec!["rupost", "--debug", "GET", "http://example.com"],
                expected_debug: true,
                expected_debug_on_failure: false,
                expected_env: None,
                expected_has_subcommand: false,
                expected_args: vec!["GET", "http://example.com"],
            },
            TestCase {
                name: "inline_mode_post_debug_trap",
                input: vec!["rupost", "GET", "http://example.com", "--debug"],
                expected_debug: false,
                expected_debug_on_failure: false,
                expected_env: None,
                expected_has_subcommand: false,
                expected_args: vec!["GET", "http://example.com", "--debug"],
            },
        ];

        for case in cases {
            let parsed = Cli::try_parse_from(&case.input)
                .unwrap_or_else(|e| panic!("Failed to parse in case '{}': {}", case.name, e));

            assert_eq!(
                parsed.debug, case.expected_debug,
                "Case '{}' debug flag mismatch",
                case.name
            );
            assert_eq!(
                parsed.debug_on_failure, case.expected_debug_on_failure,
                "Case '{}' debug_on_failure flag mismatch",
                case.name
            );
            assert_eq!(
                parsed.env.as_deref(),
                case.expected_env,
                "Case '{}' env mismatch",
                case.name
            );
            assert_eq!(
                parsed.command.is_some(),
                case.expected_has_subcommand,
                "Case '{}' subcommand presence mismatch",
                case.name
            );
            assert_eq!(
                parsed.args, case.expected_args,
                "Case '{}' args mismatch",
                case.name
            );
        }
    }

    /// 表面契约（解耦提案 Phase 4 前置锁定）：别名解析、Web value_delimiter 与
    /// 自定义 -d 短参、根命令 trailing_var_arg 捕获。迁移期间这些维度不得漂移。
    #[test]
    fn test_cli_surface_contract_aliases_and_delimiters() {
        // 1. 全部子命令别名解析到正确变体（各附最小合法参数）
        let cases: Vec<(&str, Vec<&str>)> = vec![
            ("t", vec!["x.http"]),
            ("h", vec!["l"]),
            ("g", vec!["out.http"]),
            ("d", vec!["https://example.com"]),
            ("m", vec!["snap.json"]),
            ("i", vec![]),
            ("r", vec!["snap.json"]),
            ("exp", vec!["curl", "x.http"]),
        ];
        for (alias, tail) in cases {
            let mut input = vec!["rupost", alias];
            input.extend(tail);
            let cli = Cli::try_parse_from(&input)
                .unwrap_or_else(|e| panic!("alias {alias} must parse: {e}"));
            assert!(
                cli.command.is_some(),
                "alias {alias} must yield a subcommand"
            );
        }

        // 2. Init 双别名 i / template
        assert!(Cli::try_parse_from(["rupost", "i"]).is_ok());
        assert!(Cli::try_parse_from(["rupost", "template"]).is_ok());

        // 3. Web --allowed-hosts 逗号分隔多值（value_delimiter = ','）
        let cli = Cli::try_parse_from([
            "rupost",
            "web",
            "--allowed-hosts",
            "a.example.com,b.example.com",
        ])
        .expect("web delimiter must parse");
        let Some(Commands::Web(args)) = cli.command else {
            panic!("expected Web subcommand");
        };
        let WebArgs { allowed_hosts, .. } = args;
        assert_eq!(
            allowed_hosts,
            vec!["a.example.com".to_string(), "b.example.com".to_string()],
            "comma-separated hosts must split via value_delimiter"
        );

        // 4. Web 自定义短参 -d（与全局 --debug 无短参语义不冲突）
        let cli =
            Cli::try_parse_from(["rupost", "web", "-d", "./docs"]).expect("web -d must parse");
        let Some(Commands::Web(args)) = cli.command else {
            panic!("expected Web subcommand");
        };
        let WebArgs { dir, .. } = args;
        assert_eq!(dir, "./docs");

        // 5. 根命令 trailing_var_arg + allow_hyphen_values：内联请求整体入 args
        let cli = Cli::try_parse_from(["rupost", "GET", "http://example.com", "-H", "X: 1"])
            .expect("inline request must parse via trailing var arg");
        assert!(cli.command.is_none());
        assert_eq!(cli.args, vec!["GET", "http://example.com", "-H", "X: 1"]);
    }

    /// R2 验收（TUI 流式调试 PRD）：`tui` 子命令的 --env/--var/--env-file 解析
    /// 与 TuiOptions 组装映射。变量允许多次 --var 累积；file 为可选位置参数。
    #[test]
    fn test_tui_args_parse_and_options_mapping() {
        let cli = Cli::try_parse_from([
            "rupost",
            "tui",
            "suite.http",
            "--env",
            "staging",
            "--var",
            "k1=v1",
            "--var",
            "k2=v2",
            "--env-file",
            ".env.staging",
            "--cookie-file",
            "cookies_tui.json",
        ])
        .expect("tui args must parse");

        let Some(Commands::Tui(args)) = cli.command else {
            panic!("expected Tui subcommand");
        };
        let TuiArgs {
            file,
            env,
            var,
            env_file,
            cookie_file,
        } = args;
        {
            assert_eq!(file.as_deref(), Some("suite.http"));
            assert_eq!(env.as_deref(), Some("staging"));
            assert_eq!(var, vec!["k1=v1".to_string(), "k2=v2".to_string()]);
            assert_eq!(env_file.as_deref(), Some(".env.staging"));
            assert_eq!(cookie_file.as_deref(), Some("cookies_tui.json"));

            // main.rs 分发处的 TuiOptions 组装映射（字段一一对应）
            let opts = rupost::tui::TuiOptions {
                env: env.clone(),
                vars: var.clone(),
                env_file: env_file.clone(),
                cookie_file: cookie_file.clone(),
            };
            assert_eq!(opts.env.as_deref(), Some("staging"));
            assert_eq!(opts.vars, vec!["k1=v1".to_string(), "k2=v2".to_string()]);
            assert_eq!(opts.env_file.as_deref(), Some(".env.staging"));
            assert_eq!(opts.cookie_file.as_deref(), Some("cookies_tui.json"));
        }
    }

    /// R2：无参 `tui` 的默认值（file/env/env_file 均空、vars 空集）
    #[test]
    fn test_tui_defaults_without_file() {
        let cli = Cli::try_parse_from(["rupost", "tui"]).expect("bare tui must parse");
        let Some(Commands::Tui(args)) = cli.command else {
            panic!("expected Tui subcommand");
        };
        let TuiArgs {
            file,
            env,
            var,
            env_file,
            cookie_file,
        } = args;
        assert!(file.is_none());
        assert!(env.is_none());
        assert!(var.is_empty());
        assert!(env_file.is_none());
        assert!(cookie_file.is_none());
    }

    #[test]
    fn test_web_command_parsing_and_defaults() {
        // 默认参数
        let cli_default = Cli::try_parse_from(["rupost", "web"]).expect("bare web must parse");
        let Some(Commands::Web(args)) = cli_default.command else {
            panic!("expected Web subcommand");
        };
        let WebArgs {
            port,
            host,
            static_dir,
            readonly,
            allowed_hosts,
            dir,
            dev,
        } = args;
        assert_eq!(port, 3000);
        assert_eq!(host, "127.0.0.1");
        assert!(static_dir.is_none());
        assert!(!readonly);
        assert!(!dev);
        assert!(allowed_hosts.is_empty());
        assert_eq!(dir, ".");

        // 自定义全量参数
        let cli_custom = Cli::try_parse_from([
            "rupost",
            "--env",
            "staging",
            "web",
            "--port",
            "8080",
            "--host",
            "0.0.0.0",
            "--static-dir",
            "./dist",
            "--readonly",
            "--dir",
            "./docs",
        ])
        .expect("custom web args must parse");

        assert_eq!(cli_custom.env.as_deref(), Some("staging"));
        let Some(Commands::Web(args)) = cli_custom.command else {
            panic!("expected Web subcommand");
        };
        let WebArgs {
            port,
            host,
            static_dir,
            readonly,
            allowed_hosts,
            dir,
            ..
        } = args;
        assert_eq!(port, 8080);
        assert_eq!(host, "0.0.0.0");
        assert_eq!(static_dir.as_deref(), Some("./dist"));
        assert!(readonly);
        assert!(allowed_hosts.is_empty());
        assert_eq!(dir, "./docs");
    }

    /// --dev 旗标解析（执行层的回环强制在 run_web_server，集成测试覆盖）
    #[test]
    fn test_web_dev_flag_parse() {
        let cli = Cli::try_parse_from(["rupost", "web", "--dev"]).expect("bare --dev must parse");
        let Some(Commands::Web(args)) = cli.command else {
            panic!("expected Web subcommand");
        };
        assert!(args.dev);

        // 非 dev 缺省
        let cli = Cli::try_parse_from(["rupost", "web"]).expect("bare web must parse");
        let Some(Commands::Web(args)) = cli.command else {
            panic!("expected Web subcommand");
        };
        assert!(!args.dev);
    }
}
