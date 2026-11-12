//! Step 2 TDD 守护测试：PluginPipeline 阶段上下文串联、SuiteHookCache 键隔离、
//! requires_env 环境变量放行与阻断错误文案一致性。

mod common;

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
    directives: &str,
) -> std::path::PathBuf {
    let exe = common::mock_plugin::install_mock_executable(
        root,
        &format!("rupost-plugin-{name}"),
        directives,
    );
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
        runtime: None,
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
        "READ_STDIN=\nSTDOUT_LINE={\"protocol_version\":\"1.0\",\"extra_headers\":[[\"Authorization\",\"Bearer stage1\"]],\"remove_headers\":[],\"mutated_body\":\"{\\\"signed_payload\\\":1}\",\"set_variables\":{\"TOKEN\":\"tok-1\"},\"warnings\":[]}\n",
    );
    let sign_dump = dir.path().join("sign_stdin.json");
    let sign_exe = install_script_plugin(
        dir.path(),
        "signer",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec![],
        &format!(
            "CAPTURE_STDIN={}\nSTDOUT_LINE={{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Sig\",\"ok\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}\n",
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
            "APPEND_STDIN={}\nSTDOUT_LINE={{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Env\",\"ok\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}\n",
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
        std::env::set_var("CUSTOM_SIGN_SECRET_STEP2", "custom_secret_42");
    }

    // 1. RUPOST_* 变量默认放行
    let exe_rupost = install_script_plugin(
        dir.path(),
        "env-reader",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec!["RUPOST_TEST_REQ_TOKEN".to_string()],
        "READ_STDIN=\nSTDOUT_LINE={\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Tok\",\"$ENV{RUPOST_TEST_REQ_TOKEN}\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{},\"warnings\":[]}\n",
    );
    // 2. 任意 requires_env 变量未经 --plugin-env 显式放行，绝不能静默从宿主机拷贝
    let exe2 = install_script_plugin(
        dir.path(),
        "custom-env",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec!["CUSTOM_SIGN_SECRET_STEP2".to_string()],
        "READ_STDIN=\nSTDOUT_LINE={\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Custom\",\"$ENV{CUSTOM_SIGN_SECRET_STEP2}\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{},\"warnings\":[]}\n",
    );

    let mut reg = PluginRegistry::new();
    for exe in [exe_rupost, exe2] {
        let m: PluginManifest =
            serde_json::from_str(&std::fs::read_to_string(exe.with_extension("json")).unwrap())
                .unwrap();
        reg.register_manifest(exe, m);
    }

    let cache = SuiteHookCache::new();

    // RUPOST_* 前缀自动透传
    let out_rupost = PluginPipeline::execute_for_request(
        &[PluginDirective::new(
            "sign",
            "env-reader",
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
    assert!(out_rupost.blocking_error.is_none());
    assert_eq!(
        out_rupost.patch.unwrap().extra_headers[0],
        ("X-Tok".to_string(), "from_parent_env".to_string())
    );

    // 未传 --plugin-env 时，即使 manifest 声明了 requires_env，宿主机 CUSTOM_SIGN_SECRET_STEP2 也不得泄漏
    let out_denied = PluginPipeline::execute_for_request(
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
    assert!(
        out_denied.blocking_error.is_none(),
        "{:?}",
        out_denied.blocking_error
    );
    assert_eq!(
        out_denied.patch.unwrap().extra_headers[0],
        ("X-Custom".to_string(), String::new()),
        "未经 --plugin-env 授权时，requires_env 不得静默从宿主机拷贝"
    );

    // 通过 --plugin-env 显式放行后，子进程才能收到该环境变量
    let authorized_opts = PluginRuntimeOptions {
        plugin_env: rupost::plugin::parse_plugin_env_specs(&[
            "CUSTOM_SIGN_SECRET_STEP2".to_string()
        ]),
        ..Default::default()
    };
    let out_env = PluginPipeline::execute_for_request(
        &[PluginDirective::new(
            "sign",
            "custom-env",
            PluginStage::Sign,
        )],
        &reg,
        &authorized_opts,
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
            "READ_STDIN=\nTOUCH={0}/a.started\nWAIT_FILE={0}/b.started|2000|41\nSTDOUT_LINE={{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Side\",\"A\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}\n",
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
            "READ_STDIN=\nTOUCH={0}/b.started\nWAIT_FILE={0}/a.started|2000|42\nSTDOUT_LINE={{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Side\",\"B\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}\n",
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
            "READ_STDIN=\nSLEEP_MS=40\nAPPEND_LINE={}|hit\nSTDOUT_LINE={{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"Authorization\",\"Bearer singleflight\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}\n",
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
            "APPEND_STDIN={}\nSTDOUT_LINE={{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Req\",\"ok\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}\n",
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

#[tokio::test]
async fn test_pipeline_malformed_patch_field_fails_with_invalid_output() {
    let dir = tempfile::tempdir().unwrap();
    let exe = install_script_plugin(
        dir.path(),
        "bad-patch",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec![],
        "READ_STDIN=\nSTDOUT_LINE={\"protocol_version\":\"1.0\",\"extra_headers\":\"not-an-array\"}\n",
    );
    let mut reg = PluginRegistry::new();
    let m: PluginManifest =
        serde_json::from_str(&std::fs::read_to_string(exe.with_extension("json")).unwrap())
            .unwrap();
    reg.register_manifest(exe, m);

    let cache = SuiteHookCache::new();
    let directive = PluginDirective::new("sign", "bad-patch", PluginStage::Sign);

    let blocked = PluginPipeline::execute_for_request(
        std::slice::from_ref(&directive),
        &reg,
        &PluginRuntimeOptions::default(),
        &cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    assert!(
        blocked
            .blocking_error
            .as_deref()
            .is_some_and(|e| e.contains("invalid_output")),
        "畸形 Patch 字段必须触发 invalid_output 阻断: {blocked:?}"
    );

    let degraded = PluginPipeline::execute_for_request(
        &[directive],
        &reg,
        &PluginRuntimeOptions {
            allow_missing_plugins: true,
            ..Default::default()
        },
        &cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    assert!(degraded.blocking_error.is_none());
    assert!(degraded.degraded);
    assert!(degraded.patch.is_some());
}

#[tokio::test]
async fn test_pipeline_multistage_same_plugin_executes_distinct_stage_hooks() {
    let dir = tempfile::tempdir().unwrap();
    let prep_calls = dir.path().join("prep_calls.txt");
    let sign_calls = dir.path().join("sign_calls.txt");
    let exe = dir.path().join("rupost-plugin-dual-hook");
    std::fs::write(
        &exe,
        format!(
            "#!/bin/sh\ninput=$(cat)\nif echo \"$input\" | grep -q '\"stage\":\"prepare\"'; then\n  echo prep >> \"{}\"\n  echo '{{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"Authorization\",\"Bearer dual\"]]}}'\nelse\n  echo sign >> \"{}\"\n  echo '{{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Dual-Sig\",\"ok\"]]}}'\nfi\n",
            prep_calls.display(),
            sign_calls.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let manifest = PluginManifest {
        manifest_version: "1.0".to_string(),
        name: "dual-hook".to_string(),
        version: "0.1.0".to_string(),
        description: "dual stage".to_string(),
        commands: vec![],
        hooks: vec![
            HookCapability {
                directive: "auth".to_string(),
                alias: Some("dual-hook".to_string()),
                stage: PluginStage::Prepare,
                cache_scope: PluginCacheScope::Suite,
                requires_env: vec![],
                parameters_schema: serde_json::json!({}),
            },
            HookCapability {
                directive: "sign".to_string(),
                alias: Some("dual-hook".to_string()),
                stage: PluginStage::Sign,
                cache_scope: PluginCacheScope::None,
                requires_env: vec![],
                parameters_schema: serde_json::json!({}),
            },
        ],
        runtime: None,
    };
    let mut reg = PluginRegistry::new();
    reg.register_manifest(exe, manifest);
    assert!(reg.warnings.is_empty(), "{:?}", reg.warnings);

    let directives = vec![
        PluginDirective::new("auth", "dual-hook", PluginStage::Prepare),
        PluginDirective::new("sign", "dual-hook", PluginStage::Sign),
    ];
    let cache = SuiteHookCache::new();
    let opts = PluginRuntimeOptions::default();

    for _ in 0..2 {
        let out = PluginPipeline::execute_for_request(
            &directives,
            &reg,
            &opts,
            &cache,
            "http://x",
            "GET",
            &[],
            None,
        )
        .await;
        assert!(out.blocking_error.is_none(), "{:?}", out.blocking_error);
        let headers = out.patch.unwrap().extra_headers;
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer dual")
        );
        assert!(headers.iter().any(|(k, v)| k == "X-Dual-Sig" && v == "ok"));
    }

    assert_eq!(
        std::fs::read_to_string(&prep_calls)
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(&sign_calls)
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[tokio::test]
async fn test_pipeline_preflight_runtime_missing_degrades_with_prescription_without_spawning() {
    use rupost::plugin::types::PluginRuntime;

    let dir = tempfile::tempdir().unwrap();
    let sentinel = dir.path().join("should_never_spawn.txt");
    let exe = dir.path().join("rupost-plugin-py-missing");
    std::fs::write(
        &exe,
        format!("#!/bin/sh\ntouch \"{}\"\nexit 99\n", sentinel.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let manifest = PluginManifest {
        manifest_version: "1.0".to_string(),
        name: "py-missing".to_string(),
        version: "0.1.0".to_string(),
        description: "needs missing interpreter".to_string(),
        commands: vec![],
        hooks: vec![HookCapability {
            directive: "sign".to_string(),
            alias: Some("py-missing".to_string()),
            stage: PluginStage::Sign,
            cache_scope: PluginCacheScope::None,
            requires_env: vec![],
            parameters_schema: serde_json::json!({}),
        }],
        runtime: Some(PluginRuntime {
            interpreter: Some("rupost_missing_python_999".to_string()),
            min_version: Some("3.10".to_string()),
            modules: vec!["boto3".to_string()],
        }),
    };
    let mut reg = PluginRegistry::new();
    reg.register_manifest(exe, manifest);

    let cache = SuiteHookCache::new();
    let directive = PluginDirective::new("sign", "py-missing", PluginStage::Sign);

    let blocked = PluginPipeline::execute_for_request(
        std::slice::from_ref(&directive),
        &reg,
        &PluginRuntimeOptions::default(),
        &cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    assert!(!sentinel.exists(), "预检失败时绝不能拉起插件子进程");
    let block_msg = blocked.blocking_error.expect("blocking error expected");
    assert!(block_msg.contains("runtime_missing"), "{block_msg}");
    assert!(block_msg.contains("①"), "{block_msg}");
    assert!(block_msg.contains("②"), "{block_msg}");
    assert!(block_msg.contains("③"), "{block_msg}");
    assert!(block_msg.contains("--allow-missing-plugins"), "{block_msg}");
    assert!(!block_msg.contains("已降级跳过"), "{block_msg}");

    let degraded = PluginPipeline::execute_for_request(
        &[directive],
        &reg,
        &PluginRuntimeOptions {
            allow_missing_plugins: true,
            ..Default::default()
        },
        &cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    assert!(!sentinel.exists());
    assert!(degraded.blocking_error.is_none());
    assert!(degraded.degraded);
    assert!(
        degraded.warnings.iter().any(|w| {
            w.contains("已降级跳过")
                && w.contains("rupost_missing_python_999")
                && w.contains("①")
                && w.contains("②")
                && w.contains("③")
        }),
        "降级警告必须包含三选一处方: {:?}",
        degraded.warnings
    );
}

#[tokio::test]
async fn test_pipeline_dependency_missing_attaches_three_option_prescription() {
    let dir = tempfile::tempdir().unwrap();
    let exe = install_script_plugin(
        dir.path(),
        "dep-missing",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec![],
        "READ_STDIN=\nSTDERR_LINE=ModuleNotFoundError: No module named 'boto3'\nEXIT_CODE=1\n",
    );
    let mut reg = PluginRegistry::new();
    let m: PluginManifest =
        serde_json::from_str(&std::fs::read_to_string(exe.with_extension("json")).unwrap())
            .unwrap();
    reg.register_manifest(exe, m);

    let cache = SuiteHookCache::new();
    let directive = PluginDirective::new("sign", "dep-missing", PluginStage::Sign);

    let blocked = PluginPipeline::execute_for_request(
        std::slice::from_ref(&directive),
        &reg,
        &PluginRuntimeOptions::default(),
        &cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    let block_msg = blocked.blocking_error.expect("blocking error");
    assert!(block_msg.contains("dependency_missing"), "{block_msg}");
    assert!(block_msg.contains("boto3"), "{block_msg}");
    assert!(block_msg.contains("①"), "{block_msg}");
    assert!(block_msg.contains("②"), "{block_msg}");
    assert!(block_msg.contains("③"), "{block_msg}");

    let degraded = PluginPipeline::execute_for_request(
        &[directive],
        &reg,
        &PluginRuntimeOptions {
            allow_missing_plugins: true,
            ..Default::default()
        },
        &cache,
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;
    assert!(degraded.blocking_error.is_none());
    assert!(degraded.degraded);
    assert!(
        degraded
            .warnings
            .iter()
            .any(|w| w.contains("boto3") && w.contains("①") && w.contains("②") && w.contains("③")),
        "依赖缺失降级警告必须包含模块名与三选一处方: {:?}",
        degraded.warnings
    );
}

/// D1/D5 收口（审查发现）：request 作用域下降级后必须写入"降级哨兵"——
/// 同一请求内重复声明同一失败指令只拉起一次子进程、只产生一条降级告警，
/// 绝不重复调用与告警轰炸（跨请求仍按各自现场重试，suite 作用域自愈语义不变）。
#[tokio::test]
async fn test_pipeline_request_scope_degraded_sentinel_dedupes_retries() {
    let dir = tempfile::tempdir().unwrap();
    let counter_file = dir.path().join("sentinel_calls.txt");
    let exe = install_script_plugin(
        dir.path(),
        "flaky-auth",
        PluginStage::Prepare,
        PluginCacheScope::Request,
        vec![],
        &format!(
            "READ_STDIN=\nAPPEND_STDIN={}\nEXIT_CODE=3\n",
            counter_file.display()
        ),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut reg = PluginRegistry::new();
    let m: PluginManifest =
        serde_json::from_str(&std::fs::read_to_string(exe.with_extension("json")).unwrap())
            .unwrap();
    reg.register_manifest(exe, m);

    let directive = PluginDirective::new("auth", "flaky-auth", PluginStage::Prepare);
    let opts = PluginRuntimeOptions {
        allow_missing_plugins: true,
        ..PluginRuntimeOptions::default()
    };
    let outcome = PluginPipeline::execute_for_request(
        &[directive.clone(), directive],
        &reg,
        &opts,
        &SuiteHookCache::new(),
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;

    assert!(outcome.degraded, "allow_missing_plugins 下失败必须降级放行");
    let degraded_warnings = outcome
        .warnings
        .iter()
        .filter(|w| w.contains("flaky-auth"))
        .count();
    assert_eq!(
        degraded_warnings, 1,
        "同请求重复声明同一失败指令必须只产生一条降级告警: {:?}",
        outcome.warnings
    );
    let lines = std::fs::read_to_string(&counter_file).unwrap();
    assert_eq!(
        lines.lines().count(),
        1,
        "降级哨兵生效：失败指令同请求内只调用一次子进程"
    );
}

/// D4 收口（审查发现）：阻断（blocking_error）时已成功插件产出的 patch.warnings
/// 必须并入 outcome.warnings——现场信息绝不静默丢弃。
#[tokio::test]
async fn test_pipeline_blocking_preserves_upstream_patch_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let exe = install_script_plugin(
        dir.path(),
        "warn-auth",
        PluginStage::Prepare,
        PluginCacheScope::None,
        vec![],
        "READ_STDIN=\nSTDOUT_LINE={\"protocol_version\":\"1.0\",\"extra_headers\":[],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{},\"warnings\":[\"PLUGIN_WARN_1\"]}\n",
    );
    let mut reg = PluginRegistry::new();
    let m: PluginManifest =
        serde_json::from_str(&std::fs::read_to_string(exe.with_extension("json")).unwrap())
            .unwrap();
    reg.register_manifest(exe, m);

    let outcome = PluginPipeline::execute_for_request(
        &[
            PluginDirective::new("auth", "warn-auth", PluginStage::Prepare),
            PluginDirective::new("sign", "no-such-blocker", PluginStage::Sign),
        ],
        &reg,
        &PluginRuntimeOptions::default(),
        &SuiteHookCache::new(),
        "http://x",
        "GET",
        &[],
        None,
    )
    .await;

    assert!(outcome.blocking_error.is_some(), "必选插件缺失必须阻断");
    assert!(
        outcome.warnings.iter().any(|w| w.contains("PLUGIN_WARN_1")),
        "阻断时已成功插件的 patch.warnings 必须保留在现场: {:?}",
        outcome.warnings
    );
}
