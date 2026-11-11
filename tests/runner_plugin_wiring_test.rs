//! Step 4 守护测试：Runner 全场景插件装配、并行并发与 Cookie/变量回流、
//! 无注册表/早退诊断防丢失、`--plugin-env KEY`、串行 @teardown source 传递及内联失败退出码。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use rupost::parser::HttpFileParser;
use rupost::plugin::PluginRegistry;
use rupost::plugin::pipeline::PluginRuntimeOptions;
use rupost::plugin::types::{HookCapability, PluginCacheScope, PluginManifest, PluginStage};
use rupost::runner::{
    BatchExecutor, BatchMode, BatchRunRequest, TestExecutor, TestSuiteOptions, run_test_suite,
};
use rupost::variable::VariableContext;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write_exec_script(dir: &Path, file_name: &str, body: &str) -> PathBuf {
    let exe = dir.join(file_name);
    std::fs::write(&exe, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    exe
}

fn install_hook_plugin(
    dir: &Path,
    name: &str,
    directive: &str,
    stage: PluginStage,
    cache_scope: PluginCacheScope,
    requires_env: Vec<String>,
    script_body: &str,
) -> (PathBuf, PluginManifest) {
    let exe = write_exec_script(dir, &format!("rupost-plugin-{name}"), script_body);
    let manifest = PluginManifest {
        manifest_version: "1.0".to_string(),
        name: name.to_string(),
        version: "0.1.0".to_string(),
        description: "wiring-test".to_string(),
        commands: vec![],
        hooks: vec![HookCapability {
            directive: directive.to_string(),
            alias: Some(name.to_string()),
            stage,
            cache_scope,
            requires_env,
            parameters_schema: serde_json::json!({}),
        }],
    };
    let manifest_path = dir.join(format!("rupost-plugin-{name}.json"));
    std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();
    (exe, manifest)
}

#[tokio::test]
async fn test_parallel_batch_runs_plugins_and_shares_suite_cache() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(header("Authorization", "Bearer suite-tok"))
        .and(header("X-Signed", "sig-ok"))
        .respond_with(ResponseTemplate::new(200))
        .expect(2)
        .mount(&server)
        .await;

    let plugin_dir = tempfile::tempdir().unwrap();
    let auth_calls = plugin_dir.path().join("auth_calls.txt");
    let (auth_exe, auth_manifest) = install_hook_plugin(
        plugin_dir.path(),
        "p-auth",
        "auth",
        PluginStage::Prepare,
        PluginCacheScope::Suite,
        vec![],
        &format!(
            "#!/bin/sh\ncat > /dev/null\nsleep 0.03\necho call >> \"{}\"\necho '{{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"Authorization\",\"Bearer suite-tok\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}'\n",
            auth_calls.display()
        ),
    );
    let (sign_exe, sign_manifest) = install_hook_plugin(
        plugin_dir.path(),
        "p-sign",
        "sign",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec![],
        "#!/bin/sh\ncat > /dev/null\necho '{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Signed\",\"sig-ok\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{},\"warnings\":[]}'\n",
    );

    let mut reg = PluginRegistry::new();
    reg.register_manifest(auth_exe, auth_manifest);
    reg.register_manifest(sign_exe, sign_manifest);

    let suite_dir = tempfile::tempdir().unwrap();
    let fa = suite_dir.path().join("a.http");
    let fb = suite_dir.path().join("b.http");
    for f in [&fa, &fb] {
        std::fs::write(
            f,
            format!(
                "### req\nGET {}/items\n@auth p-auth\n@sign p-sign\n\n# @assert status == 200\n",
                server.uri()
            ),
        )
        .unwrap();
    }

    let parsed_a = HttpFileParser::parse_file(&fa).unwrap();
    let parsed_b = HttpFileParser::parse_file(&fb).unwrap();
    let mut files_map = HashMap::new();
    files_map.insert(fa.clone(), parsed_a);
    files_map.insert(fb.clone(), parsed_b);

    let executor = TestExecutor::new().with_plugins(Arc::new(reg), PluginRuntimeOptions::default());
    let batch_executor = BatchExecutor::new(executor);
    let mut context = VariableContext::new();
    let results = batch_executor
        .execute_batch(BatchRunRequest {
            execution_order: vec![fa, fb],
            dependencies: HashMap::new(),
            files_map,
            context: &mut context,
            mode: BatchMode::Parallel,
            concurrency: 2,
            fail_fast: false,
        })
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    for (_, list) in &results {
        assert!(list.iter().all(|r| r.success), "parallel failed: {list:?}");
    }
    let auth_count = std::fs::read_to_string(&auth_calls)
        .unwrap()
        .lines()
        .count();
    assert_eq!(
        auth_count, 1,
        "并行模式下两个 worker 必须共享 Suite 级 @auth 缓存（仅执行 1 次），实际: {auth_count}"
    );
    server.verify().await;
}

#[tokio::test]
async fn test_parallel_batch_passes_session_cookie_to_teardown_without_disk_leak() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("Set-Cookie", "sid=parallel_session; Path=/"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/public"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/session"))
        .and(header("cookie", "sid=parallel_session"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let f_login = dir.path().join("01_login.http");
    let f_public = dir.path().join("02_public.http");
    let cookie_file = dir.path().join("cookies.json");

    std::fs::write(
        &f_login,
        format!(
            "### login\nPOST {0}/login\n\n# @assert status == 200\n\n### logout\n# @teardown\nDELETE {0}/session\n\n# @assert status == 204\n",
            server.uri()
        ),
    )
    .unwrap();
    // 第二个无关并行文件不产生 Cookie（空 CookieStore）；不得覆盖 01_login 的会话 Cookie
    std::fs::write(
        &f_public,
        format!(
            "### public\nGET {}/public\n\n# @assert status == 200\n",
            server.uri()
        ),
    )
    .unwrap();

    let mut files_map = HashMap::new();
    files_map.insert(
        f_login.clone(),
        HttpFileParser::parse_file(&f_login).unwrap(),
    );
    files_map.insert(
        f_public.clone(),
        HttpFileParser::parse_file(&f_public).unwrap(),
    );

    let executor = TestExecutor::with_cookies(cookie_file.clone()).unwrap();
    let batch_executor = BatchExecutor::new(executor);
    let mut context = VariableContext::new();
    let results = batch_executor
        .execute_batch(BatchRunRequest {
            execution_order: vec![f_login, f_public],
            dependencies: HashMap::new(),
            files_map,
            context: &mut context,
            mode: BatchMode::Parallel,
            concurrency: 2,
            fail_fast: false,
        })
        .await
        .unwrap();

    for (_, list) in &results {
        assert!(
            list.iter().all(|r| r.success),
            "parallel teardown with cookie must pass: {list:?}"
        );
    }
    assert!(
        !cookie_file.exists(),
        "并行模式执行后不得将 worker Cookie 持久化到磁盘"
    );
    drop(batch_executor);
    if cookie_file.exists() {
        let on_disk = std::fs::read_to_string(&cookie_file).unwrap();
        assert!(
            !on_disk.contains("parallel_session"),
            "Parallel mode Drop 后也不得把 worker Cookie 刷入磁盘: {on_disk}"
        );
    }
    server.verify().await;
}

#[tokio::test]
async fn test_executor_without_registry_still_enforces_missing_plugin_and_degradation() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("no_reg.http");
    std::fs::write(&f, "### req\nGET http://127.0.0.1:1/x\n@sign absent-plug\n").unwrap();
    let parsed = HttpFileParser::parse_file(&f).unwrap();

    // 1. plugin_registry == None 且未允许缺失：必须阻断失败，绝不静默跳过插件
    let strict_exec = TestExecutor::new();
    let mut ctx1 = VariableContext::new();
    let res1 = strict_exec
        .execute_all(parsed.clone(), &mut ctx1)
        .await
        .unwrap();
    assert!(
        !res1[0].success,
        "缺少插件注册表时不得静默成功: {:?}",
        res1[0]
    );
    assert!(
        res1[0]
            .error
            .as_deref()
            .unwrap_or("")
            .contains("absent-plug"),
        "阻断报错应点明缺失插件名: {:?}",
        res1[0].error
    );

    // 2. allow_missing_plugins = true 时即使注册表为 None 也走完整降级标记
    let mut allow_exec = TestExecutor::new();
    allow_exec.plugin_options.allow_missing_plugins = true;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    std::fs::write(
        &f,
        format!("### req\nGET {}/ok\n@sign absent-plug\n", server.uri()),
    )
    .unwrap();
    let parsed2 = HttpFileParser::parse_file(&f).unwrap();
    let mut ctx2 = VariableContext::new();
    let res2 = allow_exec.execute_all(parsed2, &mut ctx2).await.unwrap();
    assert!(res2[0].success && res2[0].degraded, "{:?}", res2[0]);
    assert!(
        res2[0].warnings.iter().any(|w| w.contains("未安装")),
        "降级告警必须写入 result.warnings: {:?}",
        res2[0].warnings
    );
}

#[tokio::test]
async fn test_early_return_paths_preserve_diagnostics_and_duration() {
    let plugin_dir = tempfile::tempdir().unwrap();
    let (fail_exe, fail_manifest) = install_hook_plugin(
        plugin_dir.path(),
        "diag-fail",
        "sign",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec![],
        "#!/bin/sh\ncat > /dev/null\necho 'stderr-trace-info' 1>&2\nexit 7\n",
    );
    let mut reg = PluginRegistry::new();
    reg.register_manifest(fail_exe, fail_manifest);
    let reg = Arc::new(reg);

    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("diag.http");

    // 1. blocking_error 早退：保留 stderr_tail 警告、unresolved 与 plugin_duration_us
    std::fs::write(
        &f,
        "### blocked\nGET http://127.0.0.1:1/{{missing_var}}\n@sign diag-fail\n",
    )
    .unwrap();
    let parsed = HttpFileParser::parse_file(&f).unwrap();
    let exec_block = TestExecutor::new().with_plugins(reg.clone(), PluginRuntimeOptions::default());
    let mut ctx1 = VariableContext::new();
    let res_block = exec_block.execute_all(parsed, &mut ctx1).await.unwrap();
    assert!(!res_block[0].success);
    assert!(
        res_block[0].plugin_duration_us.is_some(),
        "早退需保留插件耗时"
    );
    assert!(
        res_block[0]
            .warnings
            .iter()
            .any(|w| w.contains("stderr-trace-info")),
        "blocking_error 早退丢失 stderr 警告: {:?}",
        res_block[0].warnings
    );
    assert!(
        res_block[0]
            .warnings
            .iter()
            .any(|w| w.contains("missing_var")),
        "blocking_error 早退丢失未解析变量告警: {:?}",
        res_block[0].warnings
    );

    // 2. degraded + 兜底 Header 含 {{var}} 跳过早退：同样不得丢失降级原因、stderr 与耗时
    std::fs::write(
        &f,
        "### fallback_skip\nGET http://127.0.0.1:1/x\nAuthorization: Bearer {{FALLBACK_TOK}}\n@sign diag-fail ?optional\n",
    )
    .unwrap();
    let parsed_skip = HttpFileParser::parse_file(&f).unwrap();
    let mut ctx2 = VariableContext::new();
    let res_skip = exec_block
        .execute_all(parsed_skip, &mut ctx2)
        .await
        .unwrap();
    assert!(res_skip[0].skipped && res_skip[0].degraded);
    assert!(res_skip[0].plugin_duration_us.is_some());
    assert!(
        res_skip[0]
            .warnings
            .iter()
            .any(|w| w.contains("兜底 Header 含未解析"))
            && res_skip[0]
                .warnings
                .iter()
                .any(|w| w.contains("stderr-trace-info"))
            && res_skip[0]
                .warnings
                .iter()
                .any(|w| w.contains("已降级跳过")),
        "兜底跳过必须同时保留跳过提示、降级原因与 stderr 摘要: {:?}",
        res_skip[0].warnings
    );
}

#[tokio::test]
async fn test_run_test_suite_supports_plugin_env_key_without_value_via_binary() {
    // 通过子进程隔离 $PATH、HOME 与 RUPOST_TEST_INHERIT_ENV，避免污染同进程其他测试 env
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/env-check"))
        .and(header("X-Inherited", "inherited_ok_77"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let work = tempfile::tempdir().unwrap();
    let plugin_root = work.path().join("bin");
    std::fs::create_dir_all(&plugin_root).unwrap();
    let _ = install_hook_plugin(
        &plugin_root,
        "inherit-env",
        "sign",
        PluginStage::Sign,
        PluginCacheScope::None,
        vec![], // manifest 未声明 requires_env，专测 --plugin-env KEY 放行
        "#!/bin/sh\ncat > /dev/null\necho \"{\\\"protocol_version\\\":\\\"1.0\\\",\\\"extra_headers\\\":[[\\\"X-Inherited\\\",\\\"$RUPOST_TEST_INHERIT_ENV\\\"]],\\\"remove_headers\\\":[],\\\"mutated_body\\\":null,\\\"set_variables\\\":{},\\\"warnings\\\":[]}\"\n",
    );

    let suite = work.path().join("suite.http");
    std::fs::write(
        &suite,
        format!(
            "### check\nGET {}/env-check\n@sign inherit-env\n\n# @assert status == 200\n",
            server.uri()
        ),
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(work.path())
        .env("PATH", &plugin_root)
        .env("HOME", work.path())
        .env("RUPOST_HISTORY_DIR", work.path().join("hist"))
        .env("RUPOST_TEST_INHERIT_ENV", "inherited_ok_77")
        .args(["test"])
        .arg(&suite)
        .args(["--plugin-env", "RUPOST_TEST_INHERIT_ENV"])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "run_test_suite --plugin-env KEY 应成功继承环境变量，stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    server.verify().await;
}

#[tokio::test]
async fn test_serial_teardown_records_source_file_in_history() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/work"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/cleanup"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let work = tempfile::tempdir().unwrap();
    let hist_dir = work.path().join("history");
    let suite = work.path().join("teardown_source_case.http");
    std::fs::write(
        &suite,
        format!(
            "### work\nGET {0}/work\n\n# @assert status == 200\n\n### td\n# @teardown\nDELETE {0}/cleanup\n",
            server.uri()
        ),
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(work.path())
        .env("RUPOST_HISTORY_DIR", &hist_dir)
        .args(["test"])
        .arg(&suite)
        .output()
        .unwrap();
    assert!(out.status.success());

    let hist_file = hist_dir.join("history.jsonl");
    let content = std::fs::read_to_string(&hist_file).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "应记录常规请求与 teardown 共 2 条历史");
    for line in &lines {
        assert!(
            line.contains("teardown_source_case.http"),
            "常规与 @teardown 历史均应保留 source 文件名，实际: {line}"
        );
    }
}

#[tokio::test]
async fn test_inline_cli_runner_exits_nonzero_on_failure_and_emits_warnings_to_stderr() {
    let dir = tempfile::tempdir().unwrap();

    // 1. 请求根本连不上时必须返回非 0 退出码
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_port = listener.local_addr().unwrap().port();
    drop(listener);

    let fail_out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", dir.path())
        .args([
            "GET",
            &format!("http://127.0.0.1:{closed_port}/unreachable"),
        ])
        .output()
        .unwrap();
    assert!(
        !fail_out.status.success(),
        "内联请求连接失败必须以非 0 状态码退出"
    );

    // 2. 畸形 JSON body 降级提示走 stderr，不污染响应 stdout
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/echo"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok-body"))
        .mount(&server)
        .await;

    let warn_out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", dir.path())
        .args([
            &format!("{}/echo", server.uri()),
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            "{malformed-json",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&warn_out.stdout);
    let stderr = String::from_utf8_lossy(&warn_out.stderr);
    assert!(warn_out.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("ok-body"), "stdout 应保留响应: {stdout}");
    assert!(!stdout.contains("[!]"), "警告不得污染 stdout: {stdout}");
    assert!(
        stderr.contains("[!]"),
        "构建期警告必须打印到 stderr: {stderr}"
    );
}

#[tokio::test]
async fn test_parallel_batch_merges_vars_deterministically_by_execution_order() {
    let server = MockServer::start().await;
    // f1 故意慢 60ms（即使后结束，execution_order 在前），f2 立即返回（先结束，execution_order 在后）；
    // 按 execution_order 合并时，f2 的捕获值必须稳定覆盖 f1。
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(60))
                .set_body_json(serde_json::json!({ "who": "from_first" })),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fast"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "who": "from_second" })),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("01_slow.http");
    let f2 = dir.path().join("02_fast.http");
    std::fs::write(
        &f1,
        format!(
            "### slow\nGET {}/slow\n\n# @capture shared_who from body.who\n",
            server.uri()
        ),
    )
    .unwrap();
    std::fs::write(
        &f2,
        format!(
            "### fast\nGET {}/fast\n\n# @capture shared_who from body.who\n",
            server.uri()
        ),
    )
    .unwrap();

    let mut files_map = HashMap::new();
    files_map.insert(f1.clone(), HttpFileParser::parse_file(&f1).unwrap());
    files_map.insert(f2.clone(), HttpFileParser::parse_file(&f2).unwrap());

    let batch_executor = BatchExecutor::new(TestExecutor::new());
    let mut context = VariableContext::new();
    let _ = batch_executor
        .execute_batch(BatchRunRequest {
            execution_order: vec![f1, f2],
            dependencies: HashMap::new(),
            files_map,
            context: &mut context,
            mode: BatchMode::Parallel,
            concurrency: 2,
            fail_fast: false,
        })
        .await
        .unwrap();

    assert_eq!(
        context.get("shared_who").as_deref(),
        Some("from_second"),
        "并行变量回写必须按 execution_order 确定性覆盖，不得受并发完成顺序/HashMap 顺序影响"
    );
}

#[tokio::test]
async fn test_run_test_suite_empty_dir_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let outcome = run_test_suite(TestSuiteOptions {
        paths: vec![dir.path().to_string_lossy().to_string()],
        plugin_timeout_secs: Duration::from_secs(2).as_secs(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert!(!outcome.has_failure);
    assert!(outcome.batch_results.is_empty());
}
