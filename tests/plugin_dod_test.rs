//! M6-6 DoD 验收测试：
//! 1. 100 请求 `@auth` + `cache_scope=suite` 并发单飞 coalescing，插件 spawn 次数 <= 2（实测 1）。
//! 2. 延时隔离：`sleep 2` 插件 + 快速 mock + `--debug` -> `plugin_duration_us >= 1_500_000` 且 `ttfb_us < 200_000`。
//! 3. `BatchMode::Parallel` + `fail_fast` 取消慢插件进程组（含 grandchild），快速返回且子孙进程被杀死。
//! 4. `BatchMode::Parallel` 多文件多阶段（Prepare -> Transform -> Sign）并发执行。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rupost::history::read_execution_snapshot;
use rupost::parser::HttpFileParser;
use rupost::plugin::PluginRegistry;
use rupost::plugin::pipeline::PluginRuntimeOptions;
use rupost::plugin::types::{HookCapability, PluginCacheScope, PluginManifest, PluginStage};
use rupost::runner::{BatchExecutor, BatchMode, BatchRunRequest, TestExecutor};
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
    script_body: &str,
) -> (PathBuf, PluginManifest) {
    let exe = write_exec_script(dir, &format!("rupost-plugin-{name}"), script_body);
    let manifest = PluginManifest {
        manifest_version: "1.0".to_string(),
        name: name.to_string(),
        version: "0.1.0".to_string(),
        description: "dod-test".to_string(),
        commands: vec![],
        hooks: vec![HookCapability {
            directive: directive.to_string(),
            alias: Some(name.to_string()),
            stage,
            cache_scope,
            requires_env: vec![],
            parameters_schema: serde_json::json!({}),
        }],
        runtime: None,
    };
    let manifest_path = dir.join(format!("rupost-plugin-{name}.json"));
    std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();
    (exe, manifest)
}

#[tokio::test]
async fn test_dod_100_requests_suite_cache_spawns_at_most_twice() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/item"))
        .and(header("Authorization", "Bearer suite-100"))
        .respond_with(ResponseTemplate::new(200))
        .expect(100)
        .mount(&server)
        .await;

    let plugin_dir = tempfile::tempdir().unwrap();
    let spawn_log = plugin_dir.path().join("spawn.log");
    let (auth_exe, auth_manifest) = install_hook_plugin(
        plugin_dir.path(),
        "p-auth",
        "auth",
        PluginStage::Prepare,
        PluginCacheScope::Suite,
        &format!(
            "#!/bin/sh\ncat > /dev/null\necho spawn >> \"{}\"\necho '{{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"Authorization\",\"Bearer suite-100\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{{}},\"warnings\":[]}}'\n",
            spawn_log.display()
        ),
    );

    let mut registry = PluginRegistry::new();
    registry.register_manifest(auth_exe, auth_manifest);

    // 10 个文件 * 10 个请求 = 100 个请求，并行并发 8
    let work_dir = tempfile::tempdir().unwrap();
    let mut order = Vec::new();
    let mut files_map = HashMap::new();
    let mut deps = HashMap::new();

    for f in 0..10 {
        let mut content = String::new();
        for r in 0..10 {
            content.push_str(&format!(
                "### Req {f}-{r}\n# @auth p-auth\nGET {}/item\n\n",
                server.uri()
            ));
        }
        let fp = work_dir.path().join(format!("batch_{f}.http"));
        std::fs::write(&fp, &content).unwrap();
        let parsed = HttpFileParser::parse_file(&fp).unwrap();
        order.push(fp.clone());
        deps.insert(fp.clone(), vec![]);
        files_map.insert(fp, parsed);
    }

    let executor = TestExecutor::new().with_plugins(
        Arc::new(registry),
        PluginRuntimeOptions {
            allow_missing_plugins: false,
            strict_plugins: true,
            plugin_timeout: Duration::from_secs(5),
            plugin_env: Default::default(),
        },
    );
    let batch = BatchExecutor::new(executor);
    let mut ctx = VariableContext::new();

    let results = batch
        .execute_batch(BatchRunRequest {
            execution_order: order,
            dependencies: deps,
            files_map,
            context: &mut ctx,
            mode: BatchMode::Parallel,
            concurrency: 8,
            fail_fast: false,
        })
        .await
        .unwrap();

    let total_ok: usize = results
        .iter()
        .flat_map(|(_, rs)| rs.iter())
        .filter(|r| r.success)
        .count();
    assert_eq!(total_ok, 100);

    let spawns = std::fs::read_to_string(&spawn_log).unwrap();
    let count = spawns.lines().count();
    assert!(
        count <= 2,
        "100 个请求共享 suite cache，spawn 次数必须 <= 2，实际: {count}"
    );
    server.verify().await;
}

#[tokio::test]
async fn test_dod_latency_isolation_plugin_duration_vs_ttfb() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/fast"))
        .and(header("Authorization", "Bearer slow-auth"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let fake_home = dir.path().join("home");
    std::fs::create_dir_all(&fake_home).unwrap();
    let plugin_dir = dir.path().join(".rupost").join("plugins");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    install_hook_plugin(
        &plugin_dir,
        "slowauth",
        "auth",
        PluginStage::Prepare,
        PluginCacheScope::None,
        "#!/bin/sh\ncat > /dev/null\nsleep 3\necho '{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"Authorization\",\"Bearer slow-auth\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{},\"warnings\":[]}'\n",
    );

    let http_file = dir.path().join("latency.http");
    std::fs::write(
        &http_file,
        format!(
            "### Fast Server Slow Plugin\n# @auth slowauth\nGET {}/fast\n",
            server.uri()
        ),
    )
    .unwrap();
    let snap_path = dir.path().join("latency.snapshot.json");

    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .current_dir(dir.path())
        .env("HOME", &fake_home)
        .args([
            "test",
            http_file.to_str().unwrap(),
            "--debug",
            "--allow-project-plugins",
            "--save-snapshot",
            snap_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let snap = read_execution_snapshot(&snap_path).unwrap();
    assert_eq!(snap.entries.len(), 1);
    let entry = &snap.entries[0];
    let plugin_us = entry
        .plugin_duration_us
        .expect("必须记录 plugin_duration_us");
    let ttfb_us = entry
        .timing_waterfall
        .as_ref()
        .expect("必须记录 timing_waterfall")
        .ttfb_us;
    assert!(
        plugin_us >= 3_000_000,
        "plugin_duration_us 应 > 3s（06 §5 DoD 字面口径），实际 {plugin_us}"
    );
    assert!(
        ttfb_us < 100_000,
        "HTTP TTFB 不得污染插件耗时，应 < 100ms（06 §5 DoD 字面口径），实际 {ttfb_us}"
    );
    server.verify().await;
}

#[cfg(unix)]
#[tokio::test]
async fn test_dod_parallel_fail_fast_kills_slow_plugin_process_group() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/fail-now"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/after-slow"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let plugin_dir = tempfile::tempdir().unwrap();
    let grandchild_pid_file = plugin_dir.path().join("grandchild.pid");
    let (slow_exe, slow_manifest) = install_hook_plugin(
        plugin_dir.path(),
        "slowpg",
        "auth",
        PluginStage::Prepare,
        PluginCacheScope::None,
        &format!(
            "#!/bin/sh\ncat > /dev/null\nsleep 30 &\necho $! > \"{}\"\nwait\n",
            grandchild_pid_file.display()
        ),
    );

    let mut registry = PluginRegistry::new();
    registry.register_manifest(slow_exe, slow_manifest);

    let work_dir = tempfile::tempdir().unwrap();
    let fail_http = work_dir.path().join("01_fail.http");
    std::fs::write(
        &fail_http,
        format!("### Immediate Failure\nGET {}/fail-now\n", server.uri()),
    )
    .unwrap();
    let slow_http = work_dir.path().join("02_slow.http");
    std::fs::write(
        &slow_http,
        format!(
            "### Slow Plugin\n# @auth slowpg\nGET {}/after-slow\n",
            server.uri()
        ),
    )
    .unwrap();

    let mut files_map = HashMap::new();
    files_map.insert(
        fail_http.clone(),
        HttpFileParser::parse_file(&fail_http).unwrap(),
    );
    files_map.insert(
        slow_http.clone(),
        HttpFileParser::parse_file(&slow_http).unwrap(),
    );

    let mut deps = HashMap::new();
    deps.insert(fail_http.clone(), vec![]);
    deps.insert(slow_http.clone(), vec![]);

    let executor = TestExecutor::new().with_plugins(
        Arc::new(registry),
        PluginRuntimeOptions {
            allow_missing_plugins: false,
            strict_plugins: true,
            plugin_timeout: Duration::from_secs(30),
            plugin_env: Default::default(),
        },
    );
    let batch = BatchExecutor::new(executor);
    let mut ctx = VariableContext::new();

    let started = Instant::now();
    let _ = batch
        .execute_batch(BatchRunRequest {
            execution_order: vec![fail_http, slow_http],
            dependencies: deps,
            files_map,
            context: &mut ctx,
            mode: BatchMode::Parallel,
            concurrency: 2,
            fail_fast: true,
        })
        .await
        .unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "fail_fast 必须迅速中止慢插件而非等待 30s，实际耗时: {elapsed:?}"
    );

    // 若慢插件已经启动并写出了 grandchild.pid，则其子孙进程必须随进程组被 SIGKILL 清理
    if let Ok(pid_raw) = std::fs::read_to_string(&grandchild_pid_file) {
        let pid = pid_raw.trim().to_string();
        if !pid.is_empty() {
            let mut dead = false;
            for _ in 0..20 {
                let alive = Command::new("kill")
                    .args(["-0", &pid])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if !alive {
                    dead = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(dead, "grandchild PID {pid} 必须随进程组被杀死");
        }
    }
}

#[tokio::test]
async fn test_dod_parallel_multi_stage_plugins_across_multiple_files() {
    let server = MockServer::start().await;
    for route in ["/f1", "/f2", "/f3"] {
        Mock::given(method("GET"))
            .and(path(route))
            .and(header("Authorization", "Bearer multi-auth"))
            .and(header("X-Transformed", "yes"))
            .and(header("X-Signed", "sig-multi"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
    }

    let plugin_dir = tempfile::tempdir().unwrap();
    let (auth_exe, auth_manifest) = install_hook_plugin(
        plugin_dir.path(),
        "mauth",
        "auth",
        PluginStage::Prepare,
        PluginCacheScope::Suite,
        "#!/bin/sh\ncat > /dev/null\necho '{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"Authorization\",\"Bearer multi-auth\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{},\"warnings\":[]}'\n",
    );
    let (tx_exe, tx_manifest) = install_hook_plugin(
        plugin_dir.path(),
        "mtransform",
        "transform",
        PluginStage::Transform,
        PluginCacheScope::None,
        "#!/bin/sh\ncat > /dev/null\necho '{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Transformed\",\"yes\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{},\"warnings\":[]}'\n",
    );
    let (sign_exe, sign_manifest) = install_hook_plugin(
        plugin_dir.path(),
        "msign",
        "sign",
        PluginStage::Sign,
        PluginCacheScope::None,
        "#!/bin/sh\ncat > /dev/null\necho '{\"protocol_version\":\"1.0\",\"extra_headers\":[[\"X-Signed\",\"sig-multi\"]],\"remove_headers\":[],\"mutated_body\":null,\"set_variables\":{},\"warnings\":[]}'\n",
    );

    let mut registry = PluginRegistry::new();
    registry.register_manifest(auth_exe, auth_manifest);
    registry.register_manifest(tx_exe, tx_manifest);
    registry.register_manifest(sign_exe, sign_manifest);

    let work_dir = tempfile::tempdir().unwrap();
    let mut order = Vec::new();
    let mut files_map = HashMap::new();
    let mut deps = HashMap::new();
    for route in ["f1", "f2", "f3"] {
        let fp = work_dir.path().join(format!("{route}.http"));
        std::fs::write(
            &fp,
            format!(
                "### Multi {route}\n# @auth mauth\n# @transform mtransform\n# @sign msign\nGET {}/{route}\n",
                server.uri()
            ),
        )
        .unwrap();
        let parsed = HttpFileParser::parse_file(&fp).unwrap();
        order.push(fp.clone());
        deps.insert(fp.clone(), vec![]);
        files_map.insert(fp, parsed);
    }

    let executor = TestExecutor::new().with_plugins(
        Arc::new(registry),
        PluginRuntimeOptions {
            allow_missing_plugins: false,
            strict_plugins: true,
            plugin_timeout: Duration::from_secs(5),
            plugin_env: Default::default(),
        },
    );
    let batch = BatchExecutor::new(executor);
    let mut ctx = VariableContext::new();
    let results = batch
        .execute_batch(BatchRunRequest {
            execution_order: order,
            dependencies: deps,
            files_map,
            context: &mut ctx,
            mode: BatchMode::Parallel,
            concurrency: 3,
            fail_fast: false,
        })
        .await
        .unwrap();

    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|(_, rs)| rs.len() == 1 && rs[0].success));
    server.verify().await;
}
