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
    let cache = SuiteHookCache::new();
    let outcome = PluginPipeline::execute_for_request(
        &directives,
        &reg,
        &PluginRuntimeOptions::default(),
        &cache,
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

    let cache = SuiteHookCache::new();
    let opts = PluginRuntimeOptions::default();
    let _ = PluginPipeline::execute_for_request(
        &[d1.clone()],
        &reg,
        &opts,
        &cache,
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
        &cache,
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
        &cache,
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

    let cache = SuiteHookCache::new();
    let out_env = PluginPipeline::execute_for_request(
        &[PluginDirective::new(
            "sign",
            "custom-env",
            PluginStage::Sign,
        )],
        &reg,
        &PluginRuntimeOptions::default(),
        &cache,
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
        &cache,
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

#[tokio::test]
async fn test_pipeline_per_request_hooks_run_concurrently_without_global_lock() {
    // 确定性并行门禁（无耗时抖动假阳/假阴性）：
    // 若两个任务相互等待对方写出的启动标记文件，若被全局锁串行化则会死锁直至 2s 超时失败；
    // 只有真正无锁并行时才能在数毫秒内双向会合通过。
    let dir = tempfile::tempdir().unwrap();
    let exe_a = install_script_plugin(
        dir.path(),
        "barrier-a",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec![],
        &format!(
            "#!/bin/sh\ncat > /dev/null\ntouch \"{0}/a.started\"\ni=0\nwhile [ ! -f \"{0}/b.started\" ]; do\n  sleep 0.02\n  i=$((i+1))\n  if [ $i -ge 100 ]; then exit 41; fi\ndone\necho '{{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Side\",\"A\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}'\n",
            dir.path().display()
        ),
    );
    let exe_b = install_script_plugin(
        dir.path(),
        "barrier-b",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec![],
        &format!(
            "#!/bin/sh\ncat > /dev/null\ntouch \"{0}/b.started\"\ni=0\nwhile [ ! -f \"{0}/a.started\" ]; do\n  sleep 0.02\n  i=$((i+1))\n  if [ $i -ge 100 ]; then exit 42; fi\ndone\necho '{{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Side\",\"B\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}'\n",
            dir.path().display()
        ),
    );
    let mut reg = PluginRegistry::new();
    for exe in [exe_a, exe_b] {
        let m: PluginManifest =
            serde_json::from_str(&std::fs::read_to_string(exe.with_extension("json")).unwrap())
                .unwrap();
        reg.register_manifest(exe, m);
    }
    let reg = std::sync::Arc::new(reg);
    let cache = std::sync::Arc::new(SuiteHookCache::new());
    let opts = std::sync::Arc::new(PluginRuntimeOptions::default());

    let reg1 = reg.clone();
    let cache1 = cache.clone();
    let opts1 = opts.clone();
    let h1 = tokio::spawn(async move {
        PluginPipeline::execute_for_request(
            &[PluginDirective::new("sign", "barrier-a", PluginStage::Sign)],
            &reg1,
            &opts1,
            &cache1,
            "http://x",
            "GET",
            &[],
            None,
        )
        .await
    });
    let h2 = tokio::spawn(async move {
        PluginPipeline::execute_for_request(
            &[PluginDirective::new("sign", "barrier-b", PluginStage::Sign)],
            &reg,
            &opts,
            &cache,
            "http://x",
            "GET",
            &[],
            None,
        )
        .await
    });

    let (o1, o2) = tokio::join!(h1, h2);
    let o1 = o1.unwrap();
    let o2 = o2.unwrap();
    assert!(o1.blocking_error.is_none(), "A 必须并行会合: {:?}", o1);
    assert!(o2.blocking_error.is_none(), "B 必须并行会合: {:?}", o2);
}

#[tokio::test]
async fn test_pipeline_suite_hook_cold_start_singleflight_dedup() {
    let dir = tempfile::tempdir().unwrap();
    let calls_file = dir.path().join("calls.txt");
    let exe = install_script_plugin(
        dir.path(),
        "cold-auth",
        PluginStage::Prepare,
        PluginCacheScope::Suite,
        vec![],
        &format!(
            "#!/bin/sh\ncat > /dev/null\nsleep 0.04\necho hit >> \"{}\"\necho '{{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"Authorization\",\"Bearer singleflight\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}'\n",
            calls_file.display()
        ),
    );
    let mut reg = PluginRegistry::new();
    let m: PluginManifest =
        serde_json::from_str(&std::fs::read_to_string(exe.with_extension("json")).unwrap())
            .unwrap();
    reg.register_manifest(exe, m);

    let reg = std::sync::Arc::new(reg);
    let cache = std::sync::Arc::new(SuiteHookCache::new());
    let opts = std::sync::Arc::new(PluginRuntimeOptions::default());

    let mut handles = Vec::new();
    for _ in 0..8 {
        let reg = reg.clone();
        let cache = cache.clone();
        let opts = opts.clone();
        handles.push(tokio::spawn(async move {
            PluginPipeline::execute_for_request(
                &[PluginDirective::new(
                    "auth",
                    "cold-auth",
                    PluginStage::Prepare,
                )],
                &reg,
                &opts,
                &cache,
                "http://x",
                "GET",
                &[],
                None,
            )
            .await
        }));
    }
    for h in handles {
        let out = h.await.unwrap();
        assert!(out.blocking_error.is_none(), "{out:?}");
        assert_eq!(
            out.patch.unwrap().extra_headers,
            vec![(
                "Authorization".to_string(),
                "Bearer singleflight".to_string()
            )]
        );
    }
    let count = std::fs::read_to_string(&calls_file)
        .unwrap()
        .lines()
        .count();
    assert_eq!(
        count, 1,
        "8 个冷启动并发请求对同一 Suite 插件仅允许拉起 1 次进程，实际: {count}"
    );
}

#[test]
fn test_parse_plugin_env_spec_supports_key_and_key_equals_value() {
    use rupost::plugin::{parse_plugin_env_spec, parse_plugin_env_specs};

    unsafe {
        std::env::set_var("RUPOST_TEST_SPEC_KEY", "inherited_val");
        std::env::remove_var("RUPOST_TEST_MISSING_KEY");
    }
    assert_eq!(
        parse_plugin_env_spec("FOO=bar=baz"),
        Some(("FOO".to_string(), "bar=baz".to_string()))
    );
    assert_eq!(
        parse_plugin_env_spec("RUPOST_TEST_SPEC_KEY"),
        Some((
            "RUPOST_TEST_SPEC_KEY".to_string(),
            "inherited_val".to_string()
        ))
    );
    assert_eq!(parse_plugin_env_spec("RUPOST_TEST_MISSING_KEY"), None);
    assert_eq!(parse_plugin_env_spec("=bad"), None);
    assert_eq!(
        parse_plugin_env_specs(&[
            "RUPOST_TEST_SPEC_KEY".to_string(),
            "EXPLICIT=1".to_string(),
            "RUPOST_TEST_MISSING_KEY".to_string(),
        ]),
        vec![
            (
                "RUPOST_TEST_SPEC_KEY".to_string(),
                "inherited_val".to_string()
            ),
            ("EXPLICIT".to_string(), "1".to_string()),
        ]
    );
    unsafe {
        std::env::remove_var("RUPOST_TEST_SPEC_KEY");
    }
}

/// C1（M6 差距收口）：`cache_scope=request` 必须在单请求内复用 Patch（同一请求内
/// 重复声明相同指令只拉起一次子进程），且跨请求必须重新执行（与 suite 作用域对照，
/// 防止请求级缓存被错误升级为套件级——反假守护对照组为 suite 缓存测试）。
#[tokio::test]
async fn test_pipeline_request_scope_reuses_patch_within_request_only() {
    let dir = tempfile::tempdir().unwrap();
    let counter_file = dir.path().join("req_calls.txt");
    let exe = install_script_plugin(
        dir.path(),
        "req-auth",
        PluginStage::Prepare,
        PluginCacheScope::Request,
        vec![],
        &format!(
            "#!/bin/sh\ninput=$(cat)\necho \"$input\" >> \"{}\"\necho '{{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Req\",\"ok\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}'\n",
            counter_file.display()
        ),
    );
    let mut reg = PluginRegistry::new();
    let m: PluginManifest =
        serde_json::from_str(&std::fs::read_to_string(exe.with_extension("json")).unwrap())
            .unwrap();
    reg.register_manifest(exe, m);

    let directive = PluginDirective::new("auth", "req-auth", PluginStage::Prepare);
    let cache = SuiteHookCache::new();
    let opts = PluginRuntimeOptions::default();

    // 同一请求内重复声明相同指令 → 仅 1 次子进程（请求内复用）
    let _ = PluginPipeline::execute_for_request(
        &[directive.clone(), directive.clone()],
        &reg,
        &opts,
        &cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    // 跨请求 → 必须重新拉起子进程（request 作用域严禁套件级复用）
    let out2 = PluginPipeline::execute_for_request(
        &[directive],
        &reg,
        &opts,
        &cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;

    assert!(out2.blocking_error.is_none(), "{:?}", out2.blocking_error);
    let patch = out2
        .patch
        .expect("request 作用域跨请求重调后必须产出 Patch");
    assert!(
        patch
            .extra_headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("X-Req") && v == "ok"),
        "跨请求重调后 Patch 必须含插件真实注入的 Header: {:?}",
        patch.extra_headers
    );
    let lines = std::fs::read_to_string(&counter_file).unwrap();
    assert_eq!(
        lines.lines().count(),
        2,
        "request 作用域：请求内去重（1 次）+ 跨请求重调（1 次），实际子进程调用:\n{lines}"
    );
}
