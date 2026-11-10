use chrono::Utc;
use filetime::FileTime;
use reqwest::header::HeaderMap;
use rupost::history::model::{HistoryEntry, RequestSnapshot, ResponseMeta};
use std::fs::{self, File};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};
use tempfile::tempdir;

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

fn create_test_entry(id: &str, method: &str, url: &str, status: u16) -> HistoryEntry {
    HistoryEntry {
        id: id.to_string(),
        timestamp: Utc::now(),
        duration_ms: 42,
        request: RequestSnapshot {
            method: method.to_string(),
            url: url.to_string(),
            headers: HeaderMap::new(),
            body: None,
            timeout: None,
        },
        source: Some("cli".to_string()),
        response: ResponseMeta {
            status,
            headers: HeaderMap::new(),
            body: Some(format!("response body for {id}")),
        },
    }
}

fn write_history_entries(dir: &std::path::Path, entries: &[HistoryEntry]) {
    let history_file = dir.join("history.jsonl");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(history_file)
        .expect("failed to open history.jsonl");

    for entry in entries {
        let line = serde_json::to_string(entry).expect("serialize entry failed");
        writeln!(file, "{line}").expect("write line failed");
    }
}

#[test]
fn test_history_list_limit_and_reverse() {
    let temp_dir = tempdir().expect("create temp dir");
    let dir_path = temp_dir.path();

    let e1 = create_test_entry(
        "11111111-aaaa-bbbb-cccc-111111111111",
        "GET",
        "http://example.com/one",
        200,
    );
    let e2 = create_test_entry(
        "22222222-aaaa-bbbb-cccc-222222222222",
        "POST",
        "http://example.com/two",
        201,
    );
    let e3 = create_test_entry(
        "33333333-aaaa-bbbb-cccc-333333333333",
        "DELETE",
        "http://example.com/three",
        404,
    );
    write_history_entries(dir_path, &[e1, e2, e3]);

    // 1. 默认 list：全部 3 条
    let out = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "list"])
        .output()
        .expect("run history list");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("11111111"));
    assert!(stdout.contains("22222222"));
    assert!(stdout.contains("33333333"));
    assert!(stdout.contains("http://example.com/one"));
    assert!(stdout.contains("http://example.com/two"));
    assert!(stdout.contains("http://example.com/three"));

    // 2. 带 --limit 2：只取后 2 条 (e2, e3)，不包含 e1
    let out_limit = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "list", "--limit", "2"])
        .output()
        .expect("run history list --limit 2");
    assert!(out_limit.status.success());
    let stdout_limit = String::from_utf8_lossy(&out_limit.stdout);
    assert!(!stdout_limit.contains("11111111"));
    assert!(stdout_limit.contains("22222222"));
    assert!(stdout_limit.contains("33333333"));

    // 3. 带 --reverse：顺序倒转
    let out_rev = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "list", "--reverse"])
        .output()
        .expect("run history list --reverse");
    assert!(out_rev.status.success());
    let stdout_rev = String::from_utf8_lossy(&out_rev.stdout);
    let pos1 = stdout_rev.find("11111111").unwrap();
    let pos3 = stdout_rev.find("33333333").unwrap();
    // 默认 tail 是按顺序追加，reverse 之后最新的 33333333 应该出现在 11111111 前面
    assert!(
        pos3 < pos1,
        "33333333 should appear before 11111111 when reversed"
    );
}

#[test]
fn test_history_show_by_index_id_and_notfound() {
    let temp_dir = tempdir().expect("create temp dir");
    let dir_path = temp_dir.path();

    // 1. 空历史库测试
    let out_empty = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "show", "1"])
        .output()
        .expect("run history show empty");
    assert!(out_empty.status.success());
    assert!(String::from_utf8_lossy(&out_empty.stdout).contains("No history entries found"));

    let e_old = create_test_entry(
        "aaaa1111-0000-0000-0000-000000000001",
        "GET",
        "http://example.com/old",
        200,
    );
    let e_new = create_test_entry(
        "bbbb2222-0000-0000-0000-000000000002",
        "POST",
        "http://example.com/new",
        201,
    );
    write_history_entries(dir_path, &[e_old, e_new]);

    // 2. show 1: 必须命中最新的 e_new (1-indexed, 1 is the newest)
    let out_1 = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "show", "1"])
        .output()
        .expect("run history show 1");
    assert!(out_1.status.success());
    let stdout_1 = String::from_utf8_lossy(&out_1.stdout);
    assert!(stdout_1.contains("bbbb2222"));
    assert!(stdout_1.contains("http://example.com/new"));

    // 3. show 2: 命中倒数第 2 个 (e_old)
    let out_2 = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "show", "2"])
        .output()
        .expect("run history show 2");
    assert!(out_2.status.success());
    let stdout_2 = String::from_utf8_lossy(&out_2.stdout);
    assert!(stdout_2.contains("aaaa1111"));
    assert!(stdout_2.contains("http://example.com/old"));

    // 4. show 短 ID 匹配
    let out_short = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "show", "aaaa1111"])
        .output()
        .expect("run history show short ID");
    assert!(out_short.status.success());
    assert!(String::from_utf8_lossy(&out_short.stdout).contains("http://example.com/old"));

    // 5. show 越界索引
    let out_oob = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "show", "999"])
        .output()
        .expect("run history show oob");
    assert!(out_oob.status.success());
    assert!(String::from_utf8_lossy(&out_oob.stdout).contains("out of range"));

    // 6. show 未命中 ID
    let out_missing = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "show", "ffffffff-missing"])
        .output()
        .expect("run history show missing");
    assert!(out_missing.status.success());
    assert!(!String::from_utf8_lossy(&out_missing.stdout).contains("RuPost History Entry Details"));
}

#[test]
fn test_history_prune_old_logs_only() {
    let temp_dir = tempdir().expect("create temp dir");
    let dir_path = temp_dir.path();
    let logs_dir = dir_path.join("logs");
    fs::create_dir_all(&logs_dir).expect("create logs dir");

    let old_log = logs_dir.join("old_session.log");
    let new_log = logs_dir.join("new_session.log");
    File::create(&old_log).expect("create old.log");
    File::create(&new_log).expect("create new.log");

    // 将 old_log 的 mtime 设置为 10 天前
    let ten_days_ago = SystemTime::now() - Duration::from_secs(10 * 24 * 3600);
    filetime::set_file_mtime(&old_log, FileTime::from_system_time(ten_days_ago))
        .expect("set mtime old");

    // 运行 prune --days 7 (仅删除 > 7 天前的文件)
    let out = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "prune", "--days", "7"])
        .output()
        .expect("run history prune");
    assert!(out.status.success());

    assert!(!old_log.exists(), "old_log (10 days old) should be pruned");
    assert!(new_log.exists(), "new_log (current) should be retained");
}

#[test]
fn test_history_clear_semantics() {
    let temp_dir = tempdir().expect("create temp dir");
    let dir_path = temp_dir.path();
    let logs_dir = dir_path.join("logs");
    fs::create_dir_all(&logs_dir).expect("create logs dir");

    let test_log = logs_dir.join("test.log");
    File::create(&test_log).expect("create log file");
    let e = create_test_entry(
        "12345678-0000-0000-0000-000000000000",
        "GET",
        "http://example.com",
        200,
    );
    write_history_entries(dir_path, &[e]);
    let history_db = dir_path.join("history.jsonl");
    assert!(history_db.exists());

    // 1. 无 --yes 交互模式拒绝 ("n\n")
    let mut child = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "clear"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn clear");

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"n\n");
    }
    let out_abort = child.wait_with_output().expect("wait clear aborted");
    assert!(out_abort.status.success());
    assert!(String::from_utf8_lossy(&out_abort.stdout).contains("Aborted"));
    assert!(test_log.exists(), "logs should NOT be deleted on abort");
    assert!(
        history_db.exists(),
        "history.jsonl should NOT be deleted on abort"
    );

    // 2. 带 --yes：清空 logs/，但不删除 history.jsonl
    let out_yes = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "clear", "--yes"])
        .output()
        .expect("run clear --yes");
    assert!(out_yes.status.success());
    assert!(!test_log.exists(), "logs should be cleared with --yes");
    assert!(
        history_db.exists(),
        "history.jsonl should remain without --all"
    );

    // 3. 带 --yes --all：logs 与 history.jsonl 均清空
    let out_all = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "clear", "--yes", "--all"])
        .output()
        .expect("run clear --yes --all");
    assert!(out_all.status.success());
    assert!(
        !history_db.exists(),
        "history.jsonl should be cleared with --all"
    );
}

#[test]
fn test_history_export_snapshot_schema_and_placeholder() {
    // 可重放性闭环由 tests/snapshot_replay_test.rs::test_history_export_then_replay_closed_loop 守护
    let temp_dir = tempdir().expect("create temp dir");
    let dir_path = temp_dir.path();
    let out_file = temp_dir.path().join("export.json");

    // 两条历史：e1 body=None（旧条目占位导出），e2 body=Some（原样导出）
    let mut e1 = create_test_entry(
        "11111111-aaaa-bbbb-cccc-111111111111",
        "GET",
        "http://example.com/one",
        200,
    );
    e1.response.body = None;
    let e2 = create_test_entry(
        "22222222-aaaa-bbbb-cccc-222222222222",
        "POST",
        "http://example.com/two",
        201,
    );
    write_history_entries(dir_path, &[e1, e2]);

    // 桩转实（T5 搭车）：--last N = 最近 N 条条目，静默覆盖无 --force
    let out = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args([
            "history",
            "export",
            "--last",
            "2",
            "-o",
            out_file.to_str().unwrap(),
        ])
        .output()
        .expect("run history export");

    assert!(
        out.status.success(),
        "export must exit 0: got {:?}",
        out.status
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Exported snapshot:"),
        "confirmation line expected: {stdout}"
    );

    // 产物是合法快照套件：source_file 标记 history、条目数与 --last 一致
    let content = fs::read_to_string(&out_file).expect("exported file must exist");
    let suite: rupost::history::SnapshotSuite =
        serde_json::from_str(&content).expect("export output must be a valid SnapshotSuite");
    assert_eq!(suite.entries.len(), 2);
    assert_eq!(suite.source_file.as_deref(), Some("history"));
    // body=None 旧条目占位导出：条目保留、body 为既有 From 转换的占位标记文本
    assert_eq!(
        suite.entries[0].response.as_ref().unwrap().body,
        "Old snapshot: body not recorded"
    );
    // body=Some 条目原样导出
    assert_eq!(
        suite.entries[1].response.as_ref().unwrap().body,
        "response body for 22222222-aaaa-bbbb-cccc-222222222222"
    );
}

/// D10 验收：`--last N` 截断语义 = 最近 N 条**条目**（tail(n)），旧→新追加序；缺省 --last 1
#[test]
fn test_history_export_last_n_truncates() {
    let temp_dir = tempdir().expect("create temp dir");
    let dir_path = temp_dir.path();
    let out_file = temp_dir.path().join("export.json");

    let e1 = create_test_entry(
        "11111111-aaaa-bbbb-cccc-111111111111",
        "GET",
        "http://example.com/one",
        200,
    );
    let e2 = create_test_entry(
        "22222222-aaaa-bbbb-cccc-222222222222",
        "GET",
        "http://example.com/two",
        200,
    );
    let e3 = create_test_entry(
        "33333333-aaaa-bbbb-cccc-333333333333",
        "GET",
        "http://example.com/three",
        200,
    );
    write_history_entries(dir_path, &[e1, e2, e3]);

    let run_export = |extra: &[&str]| {
        let mut args = vec!["history", "export", "-o", out_file.to_str().unwrap()];
        args.extend_from_slice(extra);
        Command::new(rupost_bin())
            .env("RUPOST_HISTORY_DIR", dir_path)
            .args(&args)
            .output()
            .expect("run history export")
    };
    let read_ids = |path: &std::path::Path| -> Vec<String> {
        let content = fs::read_to_string(path).expect("exported file must exist");
        let suite: rupost::history::SnapshotSuite =
            serde_json::from_str(&content).expect("valid SnapshotSuite");
        suite.entries.into_iter().map(|e| e.id).collect()
    };

    // --last 2：只取最近 2 条（e2, e3），旧→新序
    let out = run_export(&["--last", "2"]);
    assert!(out.status.success());
    let ids = read_ids(&out_file);
    assert_eq!(
        ids,
        vec![
            "22222222-aaaa-bbbb-cccc-222222222222".to_string(),
            "33333333-aaaa-bbbb-cccc-333333333333".to_string()
        ],
        "tail(n) must keep newest N entries in chronological order"
    );

    // 缺省 --last（默认 1）：只取最新一条
    let out_default = run_export(&[]);
    assert!(out_default.status.success());
    let ids_default = read_ids(&out_file);
    assert_eq!(
        ids_default,
        vec!["33333333-aaaa-bbbb-cccc-333333333333".to_string()],
        "default --last 1 exports only the newest entry"
    );
}

#[test]
fn test_history_export_empty_history_writes_no_file() {
    let temp_dir = tempdir().expect("create temp dir");
    let dir_path = temp_dir.path();
    let out_file = temp_dir.path().join("should_not_exist.json");

    // 空历史目录（无 history.jsonl）：warn + 不写文件 + exit 0（对齐 generate 先例）
    let out = Command::new(rupost_bin())
        .env("RUPOST_HISTORY_DIR", dir_path)
        .args(["history", "export", "-o", out_file.to_str().unwrap()])
        .output()
        .expect("run history export on empty history");

    assert!(out.status.success(), "empty history export must exit 0");
    assert!(
        !out_file.exists(),
        "empty history must not write snapshot file"
    );
}

/// Secrets Masker：export --mask-secrets → headers 凭据 redact + body PII/敏感键打码；
/// schema 不变（产物仍可被 replay 加载）；无旗标保持原样（重放保真度优先，PRD D1）
#[test]
fn test_history_export_mask_secrets_flag() {
    let temp_dir = tempdir().expect("create temp dir");
    let dir_path = temp_dir.path();

    // 一条含 Authorization 头 + PII body 的历史
    let e = create_test_entry(
        "11111111-aaaa-bbbb-cccc-111111111111",
        "GET",
        "http://example.com/one",
        200,
    );
    let mut masked = e.clone();
    masked.request.headers.insert(
        reqwest::header::AUTHORIZATION,
        "Bearer sk-live-secret-value".parse().unwrap(),
    );
    masked.response.body =
        Some(r#"{"phone":"13812345678","password":"p@ss","memo":"ok"}"#.to_string());
    let raw_view = masked.clone();
    write_history_entries(dir_path, &[masked]);
    let out_masked = temp_dir.path().join("masked.json");
    let out_raw = temp_dir.path().join("raw.json");

    let run_export = |out: &std::path::Path, mask: bool| {
        let mut args = vec![
            "history".to_string(),
            "export".to_string(),
            "-o".to_string(),
            out.to_str().unwrap().to_string(),
        ];
        if mask {
            args.push("--mask-secrets".to_string());
        }
        Command::new(rupost_bin())
            .env("RUPOST_HISTORY_DIR", dir_path)
            .args(&args)
            .output()
            .expect("run history export")
    };

    let out = run_export(&out_masked, true);
    assert!(out.status.success());
    let content = fs::read_to_string(&out_masked).unwrap();
    assert!(
        content.contains("[REDACTED]"),
        "authorization header redacted: {content}"
    );
    assert!(
        !content.contains("sk-live-secret-value"),
        "credential gone: {content}"
    );
    assert!(content.contains("***"), "pii masked: {content}");
    assert!(!content.contains("13812345678"), "phone gone: {content}");
    assert!(
        content.contains("memo") && content.contains("ok"),
        "plain data preserved: {content}"
    );

    let out = run_export(&out_raw, false);
    assert!(out.status.success());
    let raw = fs::read_to_string(&out_raw).unwrap();
    assert!(
        raw.contains("sk-live-secret-value"),
        "no flag keeps fidelity (replay 依据)"
    );
    let _ = raw_view;
}
