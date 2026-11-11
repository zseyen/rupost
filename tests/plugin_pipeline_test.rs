//! Step 2 TDD 守护测试：PluginPipeline 阶段上下文串联、SuiteHookCache 键隔离、
//! requires_env 环境变量放行与阻断错误文案一致性。

use rupost::plugin::pipeline::{PluginRuntimeOptions, SuiteHookCache};
use rupost::plugin::types::{
    HookCapability, PluginCacheScope, PluginDirective, PluginManifest, PluginStage,
};
use rupost::plugin::{PluginPipeline, PluginRegistry};

fn install_script_plugin(
    root: &std::path::Path,
    name: &str,
    stage: PluginStage,
    cache_scope: PluginCacheScope,
    requires_env: Vec<String>,
    script_body: &str,
) -> std::path::PathBuf {
    std::fs::create_dir_all(root).unwrap();
    let exe = root.join(format!("rupost-plugin-{name}"));
    std::fs::write(&exe, script_body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let manifest = PluginManifest {
        manifest_version: "1.0".to_string(),
        name: name.to_string(),
        version: "0.1.0".to_string(),
        description: "test".to_string(),
        commands: vec![],
        hooks: vec![HookCapability {
            directive: match stage {
                PluginStage::Prepare => "auth".to_string(),
                PluginStage::Transform => "transform".to_string(),
                _ => "sign".to_string(),
            },
            alias: Some(name.to_string()),
            stage,
            cache_scope,
            requires_env,
            parameters_schema: serde_json::json!({}),
        }],
    };
    let mpath = root.join(format!("rupost-plugin-{name}.json"));
    std::fs::write(&mpath, serde_json::to_string(&manifest).unwrap()).unwrap();
    exe
}

#[tokio::test]
async fn test_pipeline_chains_headers_body_and_variables_across_stages() {
    let dir = tempfile::tempdir().unwrap();
    let prep_exe = install_script_plugin(
        dir.path(),
        "prep",
        PluginStage::Prepare,
        PluginCacheScope::None,
        vec![],
        r#"#!/bin/sh
cat > /dev/null
echo '{"protocol_version":"1.0","extra_headers":[["Authorization","Bearer stage1"]],"remove_headers":[],"mutated_body":"{\"signed_payload\":1}","set_variables":{"TOKEN":"tok-1"},"warnings":[]}'
"#,
    );
    let sign_dump = dir.path().join("sign_stdin.json");
    let sign_exe = install_script_plugin(
        dir.path(),
        "signer",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec![],
        &format!(
            "#!/bin/sh\ncat > \"{}\"\necho '{{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Sig\",\"ok\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}'\n",
            sign_dump.display()
        ),
    );

    let mut reg = PluginRegistry::new();
    for (exe, name) in [(prep_exe, "prep"), (sign_exe, "signer")] {
        let m: PluginManifest =
            serde_json::from_str(&std::fs::read_to_string(exe.with_extension("json")).unwrap())
                .unwrap();
        reg.register_manifest(exe, m);
        let _ = name;
    }

    let directives = vec![
        PluginDirective::new("auth", "prep", PluginStage::Prepare),
        PluginDirective::new("sign", "signer", PluginStage::Sign),
    ];
    let mut cache = SuiteHookCache::new();
    let outcome = PluginPipeline::execute_for_request(
        &directives,
        &reg,
        &PluginRuntimeOptions::default(),
        &mut cache,
        "https://api.example.com/v1",
        "POST",
        &[("X-Orig".to_string(), "1".to_string())],
        Some("raw"),
    )
    .await;

    assert!(
        outcome.blocking_error.is_none(),
        "{:?}",
        outcome.blocking_error
    );
    let captured: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&sign_dump).unwrap()).unwrap();
    assert_eq!(
        captured["request"]["body"].as_str(),
        Some("{\"signed_payload\":1}"),
        "Sign 阶段必须看到 Prepare 阶段改写后的请求体: {captured}"
    );
    let headers_json = serde_json::to_string(&captured["request"]["headers"]).unwrap();
    assert!(
        headers_json.contains("Bearer stage1"),
        "Sign 阶段必须看到 Prepare 注入的 Header: {headers_json}"
    );
    assert_eq!(
        captured["variables"]["TOKEN"].as_str(),
        Some("tok-1"),
        "Sign 阶段必须看到 Prepare 写入的变量: {captured}"
    );
}

#[tokio::test]
async fn test_pipeline_suite_cache_distinguishes_params() {
    let dir = tempfile::tempdir().unwrap();
    let counter_file = dir.path().join("calls.txt");
    let exe = install_script_plugin(
        dir.path(),
        "param-auth",
        PluginStage::Prepare,
        PluginCacheScope::Suite,
        vec![],
        &format!(
            "#!/bin/sh\ninput=$(cat)\necho \"$input\" >> \"{}\"\necho '{{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Env\",\"ok\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}'\n",
            counter_file.display()
        ),
    );
    let mut reg = PluginRegistry::new();
    let m: PluginManifest =
        serde_json::from_str(&std::fs::read_to_string(exe.with_extension("json")).unwrap())
            .unwrap();
    reg.register_manifest(exe, m);

    let mut d1 = PluginDirective::new("auth", "param-auth", PluginStage::Prepare);
    d1.params.insert("env".to_string(), "dev".to_string());
    let mut d2 = PluginDirective::new("auth", "param-auth", PluginStage::Prepare);
    d2.params.insert("env".to_string(), "prod".to_string());

    let mut cache = SuiteHookCache::new();
    let opts = PluginRuntimeOptions::default();
    let _ = PluginPipeline::execute_for_request(
        &[d1.clone()],
        &reg,
        &opts,
        &mut cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    // 同参数应命中缓存（仅增加一次 calls 不变）
    let _ = PluginPipeline::execute_for_request(
        &[d1],
        &reg,
        &opts,
        &mut cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    // 不同参数不得误命中缓存（必须触发第二次调用）
    let _ = PluginPipeline::execute_for_request(
        &[d2],
        &reg,
        &opts,
        &mut cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;

    let lines = std::fs::read_to_string(&counter_file).unwrap();
    assert_eq!(
        lines.lines().count(),
        2,
        "同参数命中缓存、不同参数隔离：实际调用次数应为 2，收到:\n{lines}"
    );
}

#[tokio::test]
async fn test_pipeline_forwards_requires_env_and_clean_blocking_error() {
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("RUPOST_TEST_REQ_TOKEN", "from_parent_env");
    }
    let exe = install_script_plugin(
        dir.path(),
        "env-reader",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec!["RUPOST_TEST_REQ_TOKEN".to_string()],
        "#!/bin/sh\ncat > /dev/null\necho \"{\\\"protocol_version\\\":\\\"1.0\\\",\\\"extra_headers\\\":[[\\\"X-Tok\\\",\\\"$RUPOST_TEST_REQ_TOKEN\\\"]],\\\"remove_headers\\\":[],\\\"mutated_body\\\":null,\\\"set_variables\\\":{},\\\"warnings\\\":[]}\"\n",
    );
    // 也测非 RUPOST_ 前缀的普通 env 名（验证 manifest requires_env 本身生效）
    unsafe {
        std::env::set_var("CUSTOM_SIGN_SECRET_STEP2", "custom_secret_42");
    }
    let exe2 = install_script_plugin(
        dir.path(),
        "custom-env",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec!["CUSTOM_SIGN_SECRET_STEP2".to_string()],
        "#!/bin/sh\ncat > /dev/null\nif [ -z \"$CUSTOM_SIGN_SECRET_STEP2\" ]; then exit 2; fi\necho \"{\\\"protocol_version\\\":\\\"1.0\\\",\\\"extra_headers\\\":[[\\\"X-Custom\\\",\\\"$CUSTOM_SIGN_SECRET_STEP2\\\"]],\\\"remove_headers\\\":[],\\\"mutated_body\\\":null,\\\"set_variables\\\":{},\\\"warnings\\\":[]}\"\n",
    );
    let _ = exe;
    let mut reg = PluginRegistry::new();
    let m2: PluginManifest =
        serde_json::from_str(&std::fs::read_to_string(exe2.with_extension("json")).unwrap())
            .unwrap();
    reg.register_manifest(exe2, m2);

    let mut cache = SuiteHookCache::new();
    let out_env = PluginPipeline::execute_for_request(
        &[PluginDirective::new(
            "sign",
            "custom-env",
            PluginStage::Sign,
        )],
        &reg,
        &PluginRuntimeOptions::default(),
        &mut cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    unsafe {
        std::env::remove_var("CUSTOM_SIGN_SECRET_STEP2");
        std::env::remove_var("RUPOST_TEST_REQ_TOKEN");
    }
    assert!(
        out_env.blocking_error.is_none(),
        "{:?}",
        out_env.blocking_error
    );
    let headers = &out_env.patch.unwrap().extra_headers;
    assert_eq!(
        headers[0],
        ("X-Custom".to_string(), "custom_secret_42".to_string())
    );

    // 必选插件缺失走 blocking_error，绝不能出现“已降级跳过”误导性告警
    let out_block = PluginPipeline::execute_for_request(
        &[PluginDirective::new(
            "sign",
            "nonexistent-plugin",
            PluginStage::Sign,
        )],
        &reg,
        &PluginRuntimeOptions::default(),
        &mut cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    assert!(out_block.blocking_error.is_some());
    assert!(
        !out_block.warnings.iter().any(|w| w.contains("已降级跳过")),
        "阻断错误不得误写“已降级跳过”告警: {:?}",
        out_block.warnings
    );
}
