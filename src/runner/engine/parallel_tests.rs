use super::*;
use crate::variable::VariableContext;

/// 构造一个已完成的前置依赖通道；返回 sender 由调用方持有，保证通道存活
#[allow(clippy::type_complexity)]
fn ready_dep(
    vars: &[(&str, &str)],
    cookie_state: Option<serde_json::Value>,
) -> (
    watch::Sender<Option<TaskOutput>>,
    watch::Receiver<Option<TaskOutput>>,
) {
    let (tx, rx) = watch::channel::<Option<TaskOutput>>(None);
    let captured_vars = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    tx.send(Some(TaskOutput {
        success: true,
        captured_vars,
        cookie_state,
    }))
    .unwrap();
    (tx, rx)
}

#[tokio::test]
async fn test_wait_for_deps_merges_vars_but_keeps_last_cookie_state() {
    let (_tx_b, dep_b) = ready_dep(&[("from_b", "1")], Some(serde_json::json!([{"name": "b"}])));
    let (_tx_c, dep_c) = ready_dep(&[("from_c", "2")], Some(serde_json::json!([{"name": "c"}])));
    let (my_tx, _my_rx) = watch::channel::<Option<TaskOutput>>(None);

    let (vars, cookie) = ParallelScheduler::wait_for_deps(vec![dep_b, dep_c], my_tx)
        .await
        .expect("deps succeeded");

    assert_eq!(vars.get("from_b").map(String::as_str), Some("1"));
    assert_eq!(vars.get("from_c").map(String::as_str), Some("2"));
    assert_eq!(cookie, Some(serde_json::json!([{"name": "c"}])));
}

#[tokio::test]
async fn test_wait_for_deps_single_cookie_parent_passes_through() {
    let (_tx_a, dep_a) = ready_dep(&[], Some(serde_json::json!([{"name": "a"}])));
    let (_tx_n, dep_no_cookie) = ready_dep(&[("x", "y")], None);
    let (my_tx, _my_rx) = watch::channel::<Option<TaskOutput>>(None);

    let (_vars, cookie) = ParallelScheduler::wait_for_deps(vec![dep_a, dep_no_cookie], my_tx)
        .await
        .expect("deps succeeded");
    assert_eq!(cookie, Some(serde_json::json!([{"name": "a"}])));
}

#[test]
fn test_extract_changed_vars() {
    let mut before = VariableContext::new();
    before.insert("a", "1");
    before.insert("b", "2");

    let mut after = before.clone();
    after.insert("b", "modified");
    after.insert("c", "3");

    let diff = ParallelScheduler::extract_changed_vars(&before, &after);
    assert_eq!(diff.len(), 2);
    assert_eq!(diff.get("b").map(|s| s.as_str()), Some("modified"));
    assert_eq!(diff.get("c").map(|s| s.as_str()), Some("3"));
}

#[test]
fn test_build_executor_with_cookie_and_plugin_state() {
    let reg = Arc::new(crate::plugin::PluginRegistry::new());
    let opts = crate::plugin::pipeline::PluginRuntimeOptions {
        allow_missing_plugins: true,
        strict_plugins: true,
        ..Default::default()
    };
    let base_executor = TestExecutor::with_ephemeral_cookies().with_plugins(reg.clone(), opts);
    let shared_cfg = WorkerExecutorConfig::from_executor(&base_executor);

    let middleware = crate::middleware::CookieMiddleware::new_ephemeral();
    {
        let store_arc = middleware.cookie_store();
        let mut store = store_arc.lock().unwrap();
        let cookie = cookie::Cookie::build(("session", "abcdef"))
            .domain("api.example.com")
            .path("/")
            .build();
        let url = url::Url::parse("https://api.example.com/").unwrap();
        store.insert_raw(&cookie, &url).unwrap();
    }
    let cookie_state = middleware.export_cookie_state().unwrap();

    let executor = ParallelScheduler::build_executor_with_state(&shared_cfg, Some(cookie_state));

    assert!(executor.has_cookies());
    assert!(executor.plugin_registry.is_some());
    assert!(executor.plugin_options.allow_missing_plugins);
    assert!(executor.plugin_options.strict_plugins);
    assert!(Arc::ptr_eq(
        &executor.suite_hook_cache,
        &base_executor.suite_hook_cache
    ));
    let current_state = executor.export_cookie_state().unwrap().unwrap();
    assert!(current_state.to_string().contains("session"));
    assert!(current_state.to_string().contains("abcdef"));
}

#[tokio::test]
async fn test_parallel_scheduler_deduplicates_duplicate_execution_order_without_panic() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("dup.http");
    std::fs::write(&f1, "### skipped\n# @skip\nGET http://127.0.0.1:1/skip\n").unwrap();
    let parsed = crate::parser::HttpFileParser::parse_file(&f1).unwrap();
    let mut files_map = HashMap::new();
    files_map.insert(f1.clone(), parsed);

    let executor = TestExecutor::new();
    let mut context = VariableContext::new();
    let scheduler = ParallelScheduler::new(
        &executor,
        vec![f1.clone(), f1.clone()],
        HashMap::new(),
        files_map,
        &mut context,
        2,
        false,
    );

    let output = scheduler
        .run()
        .await
        .expect("duplicate execution_order must not panic");
    assert_eq!(output.results.len(), 1);
    assert_eq!(output.results[0].0, f1);
}
