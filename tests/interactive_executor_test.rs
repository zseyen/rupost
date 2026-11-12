//! B-1（免重填主线）：交互入口（TUI/Web）共享执行器工厂的插件装配守护。
//!
//! 断点回顾：TUI/Web 此前裸建 `TestExecutor::with_ephemeral_cookies()`（空注册表），
//! 本机已安装的非 optional 插件指令在两入口假性报"插件未安装"阻断。
//! 工厂收口后：`build_interactive_executor` 装配真实注册表，表现层即调即走。

use rupost::parser::types::{
    ParsedPluginDirective, ParsedPluginStage, ParsedRequest, RequestMetadata,
};
use rupost::runner::executor_factory;
use std::collections::HashMap;
use std::path::PathBuf;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn install_sh_plugin(root: &std::path::Path, name: &str) -> PathBuf {
    let dir = root.join(".rupost").join("plugins");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join(format!("rupost-plugin-{name}"));
    std::fs::write(
        &exe,
        "#!/bin/sh\ncat > /dev/null\necho '{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Interactive\",\"yes\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{},\"warnings\":[]}'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let manifest = serde_json::json!({
        "manifest_version": "1.0", "name": name, "version": "0.1.0", "description": "interactive test",
        "commands": [],
        "hooks": [{ "directive": "auth", "alias": name, "stage": "prepare", "cache_scope": "none", "requires_env": [], "parameters_schema": {} }]
    });
    std::fs::write(
        dir.join(format!("rupost-plugin-{name}.json")),
        manifest.to_string(),
    )
    .unwrap();
    exe
}

fn auth_request(url: &str) -> ParsedRequest {
    let mut plugins_map = HashMap::new();
    plugins_map.insert("who".to_string(), "interactive".to_string());
    let mut req = ParsedRequest::new(1);
    req.method = Some("GET".to_string());
    req.url = url.to_string();
    req.metadata = RequestMetadata {
        name: Some("guarded".to_string()),
        plugins: vec![ParsedPluginDirective {
            directive: "auth".to_string(),
            name: "webauth".to_string(),
            stage: ParsedPluginStage::Prepare,
            optional: false,
            params: plugins_map,
        }],
        ..RequestMetadata::default()
    };
    req
}

/// 工厂装配后：本机（项目根放行）安装的插件在交互执行路径真实生效——
/// 防假守护：mock 依赖插件注入的 X-Interactive header 返回 200，否则 403。
#[tokio::test]
async fn test_interactive_executor_runs_installed_plugin() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/guarded"))
        .and(wiremock::matchers::header("X-Interactive", "yes"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/guarded"))
        .respond_with(ResponseTemplate::new(403).set_body_string("no plugin header"))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    install_sh_plugin(dir.path(), "webauth");

    let executor =
        executor_factory::build_interactive_executor_with(&rupost::plugin::DiscoverOptions {
            allow_project_plugins: true,
            project_root: Some(dir.path().to_path_buf()),
        });
    let mut ctx = rupost::variable::VariableContext::default();
    let result = executor
        .execute_one(
            auth_request(&format!("{}/guarded", server.uri())),
            1,
            &mut ctx,
            None,
        )
        .await;

    assert!(
        result.success,
        "交互执行器必须真实拉起已安装插件：status={:?} error={:?} warnings={:?}",
        result.status, result.error, result.warnings
    );
    assert_eq!(result.status, Some(200));
}

/// 对照守护：未安装插件的必选指令必须阻断且错误可读（默认门禁不旁路）。
#[tokio::test]
async fn test_interactive_executor_missing_plugin_blocks_with_readable_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/guarded"))
        .respond_with(ResponseTemplate::new(200).set_body_string("should-not-happen"))
        .mount(&server)
        .await;

    let executor = executor_factory::build_interactive_executor();
    let mut ctx = rupost::variable::VariableContext::default();
    let result = executor
        .execute_one(
            auth_request(&format!("{}/guarded", server.uri())),
            1,
            &mut ctx,
            None,
        )
        .await;

    assert!(!result.success, "未安装插件必须阻断（默认门禁）");
    let err = result.error.as_deref().unwrap_or_default();
    assert!(
        err.contains("webauth") && err.contains("未安装"),
        "阻断错误必须指名插件并给出可读原因: {err}"
    );
}
