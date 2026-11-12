use rupost::parser::HttpFileParser;
use rupost::plugin::{PluginPatch, PluginPipeline, PluginRegistry, PluginStage};
use std::path::PathBuf;

#[test]
fn test_plugin_framework_ast_and_pipeline_flow() {
    let http_content = r#"
POST https://api.my-cloud.com/v1/orders
# 书写故意对调：先写了 sign，后写了 auth
@sign echo-sign prefix=V2
@auth echo-sign key=test-secret ?optional
Content-Type: application/json

{"order_id": 123}
"#;

    let parsed = HttpFileParser::parse_content(http_content).expect("Failed to parse http file");
    assert_eq!(parsed.requests.len(), 1);

    let mut req_plugins = parsed.requests[0].metadata.plugins.clone();
    assert_eq!(req_plugins.len(), 2);

    // 1. 验证解析属性
    let sign_dir = &req_plugins[0];
    assert_eq!(sign_dir.directive, "sign");
    assert_eq!(sign_dir.name, "echo-sign");
    assert_eq!(sign_dir.stage, PluginStage::Sign);
    assert_eq!(sign_dir.params.get("prefix").unwrap(), "V2");
    assert!(!sign_dir.optional);

    let auth_dir = &req_plugins[1];
    assert_eq!(auth_dir.directive, "auth");
    assert_eq!(auth_dir.name, "echo-sign");
    assert_eq!(auth_dir.stage, PluginStage::Prepare);
    assert_eq!(auth_dir.params.get("key").unwrap(), "test-secret");
    assert!(auth_dir.optional);

    // 2. 验证 Pipeline 排序仲裁：Prepare (Stage=10) 必须先于 Sign (Stage=30)
    PluginPipeline::sort_directives(&mut req_plugins);
    assert_eq!(req_plugins[0].directive, "auth"); // 成功被仲裁到第一位
    assert_eq!(req_plugins[1].directive, "sign");

    // 3. 验证 Patch 折叠传递与合并
    let auth_patch = PluginPatch {
        extra_headers: vec![("Authorization".to_string(), "Bearer token-abc".to_string())],
        remove_headers: vec![],
        mutated_body: None,
        set_variables: std::collections::HashMap::from([(
            "TOKEN".to_string(),
            "token-abc".to_string(),
        )]),
        warnings: vec![],
    };

    let sign_patch = PluginPatch {
        extra_headers: vec![("X-Signature".to_string(), "V2-sig-xyz".to_string())],
        remove_headers: vec![],
        mutated_body: None,
        set_variables: std::collections::HashMap::new(),
        warnings: vec![],
    };

    let mut final_patch = auth_patch;
    final_patch.merge_with(sign_patch);

    assert_eq!(final_patch.extra_headers.len(), 2);
    assert_eq!(final_patch.extra_headers[0].0, "Authorization");
    assert_eq!(final_patch.extra_headers[1].0, "X-Signature");
    assert_eq!(final_patch.set_variables.get("TOKEN").unwrap(), "token-abc");
}

#[test]
fn test_plugin_registry_discovery_and_command_mapping() {
    let mut registry = PluginRegistry::new();
    let manifest = rupost::plugin::PluginManifest {
        manifest_version: "1.0".to_string(),
        name: "echo-sign".to_string(),
        version: "0.1.0".to_string(),
        description: "Echo sign demo plugin".to_string(),
        commands: vec![rupost::plugin::CommandCapability {
            name: "echo-sign".to_string(),
            usage: "rupost echo-sign --test".to_string(),
            description: "Self test".to_string(),
            requires_tty: false,
            requires_env: vec![],
        }],
        hooks: vec![rupost::plugin::HookCapability {
            directive: "auth".to_string(),
            alias: Some("echo-sign".to_string()),
            stage: PluginStage::Prepare,
            cache_scope: rupost::plugin::PluginCacheScope::default(),
            requires_env: vec![],
            parameters_schema: serde_json::json!({}),
        }],
        runtime: None,
    };

    let fake_path = PathBuf::from("/usr/local/bin/rupost-plugin-echo-sign");
    registry.register_manifest(fake_path.clone(), manifest);

    // 验证自定义 CLI 子命令与 Hook 都能通过单一注册表查找到
    let cmd = registry.find_command("echo-sign").expect("find command");
    assert_eq!(cmd.0, fake_path);

    let hook = registry.find_hook("echo-sign").expect("find hook");
    assert_eq!(hook.0, fake_path);
    assert_eq!(hook.1.stage, PluginStage::Prepare);
}
