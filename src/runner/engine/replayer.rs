#![allow(clippy::collapsible_if)]
use crate::Result;
use crate::history::SnapshotEntry;
use crate::http::{Client, Request};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

pub struct ReplayExecutor {
    client: Client,
    target_url: Option<String>,
    verbose: bool,
}

/// 单条快照重放的结构化结果（R6）：失败可定位到条，serde 双向供未来报告/triage 消费。
/// 字段形状对齐既有约定：`request_number` 1-based（同 `JsonTestReport`）、
/// `duration_ms` 毫秒 u64（同 `HistoryEntry`）、`actual_status: Option<u16>`
/// 传输层失败为 `None`（同 `FailureContext.status_code`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayEntryResult {
    /// 快照内 1-based 序号（对齐终端 `#N` 打印）
    pub request_number: usize,
    pub method: String,
    /// `--target` 覆写后实际请求的 URL（无覆写时为原始录制 URL）
    pub url: String,
    pub expected_status: u16,
    /// 传输层失败（网络/超时/请求还原失败）时为 None
    #[serde(default)]
    pub actual_status: Option<u16>,
    pub matched: bool,
    pub duration_ms: u64,
    /// 传输层失败原因（首行）；状态码 mismatch 不置 error（expected/actual 由字段承载）
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReplayReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    #[serde(default)]
    pub entries: Vec<ReplayEntryResult>,
}

impl ReplayExecutor {
    pub fn new(target_url: Option<String>, verbose: bool) -> Self {
        Self {
            client: Client::new(None),
            target_url,
            verbose,
        }
    }

    /// 执行快照重放并输出对比结果
    pub async fn replay_file<P: AsRef<Path>>(&self, path: P) -> Result<ReplayReport> {
        let suite = crate::history::read_snapshot_suite(path)?;
        println!(
            "{} Loaded snapshot containing {} requests.",
            "[*]".bold().blue(),
            suite.entries.len()
        );
        if let Some(target) = &self.target_url {
            println!(
                "{} Overriding target URL base to: {}",
                "[*]".bold().blue(),
                target.green()
            );
        }

        let mut entries: Vec<ReplayEntryResult> = Vec::with_capacity(suite.entries.len());
        let mut passed = 0;
        let mut failed = 0;
        let start_time = Instant::now();

        for (idx, entry) in suite.entries.iter().enumerate() {
            let request_number = idx + 1;
            println!(
                "\n{} Replaying request #{} [{} {}]...",
                "[*]".bold().blue(),
                request_number,
                entry.request.method.bold().magenta(),
                entry.request.url
            );

            let result = self.replay_entry(entry, request_number).await;
            if result.matched {
                passed += 1;
            } else {
                failed += 1;
            }
            entries.push(result);
        }

        let duration = start_time.elapsed();
        println!("\n{}", "─".repeat(50));
        println!(
            "{} Replay finished in {:.2?}",
            "[*]".bold().blue(),
            duration
        );
        println!(
            "  Total: {}, Passed: {}, Failed: {}",
            suite.entries.len(),
            passed.to_string().green(),
            if failed > 0 {
                failed.to_string().red()
            } else {
                failed.to_string().normal()
            }
        );

        // 失败条目清单：定位到条（仅 failed > 0 时输出；不脱敏，与内联错误行同暴露面）
        if failed > 0 {
            println!("  {}", "Failed entries:".bold().red());
            for e in entries.iter().filter(|e| !e.matched) {
                match (&e.error, e.actual_status) {
                    (Some(err), _) => {
                        println!(
                            "   - #{} {} {} (error: {})",
                            e.request_number, e.method, e.url, err
                        );
                    }
                    (None, Some(actual)) => {
                        println!(
                            "   - #{} {} {} (expected {}, got {})",
                            e.request_number, e.method, e.url, e.expected_status, actual
                        );
                    }
                    (None, None) => {
                        println!(
                            "   - #{} {} {} (expected {}, got <no response>)",
                            e.request_number, e.method, e.url, e.expected_status
                        );
                    }
                }
            }
        }

        Ok(ReplayReport {
            total: suite.entries.len(),
            passed,
            failed,
            entries,
        })
    }

    async fn replay_entry(
        &self,
        entry: &SnapshotEntry,
        request_number: usize,
    ) -> ReplayEntryResult {
        // 1. 地址覆写
        let mut target_url = entry.request.url.clone();
        if let Some(target) = &self.target_url
            && let Ok(mut parsed_url) = url::Url::parse(&entry.request.url)
            && let Ok(parsed_target) = url::Url::parse(target)
        {
            parsed_url.set_scheme(parsed_target.scheme()).ok();
            parsed_url.set_host(parsed_target.host_str()).ok();
            parsed_url.set_port(parsed_target.port()).ok();
            target_url = parsed_url.to_string();
        }

        let start = Instant::now();

        // 2. 还原 Request（失败落条目，不再 Err 分叉）
        let req = match Self::restore_request(entry, &target_url) {
            Ok(req) => req,
            Err(e) => {
                return Self::transport_error_entry(
                    entry,
                    request_number,
                    target_url,
                    start.elapsed(),
                    &e,
                );
            }
        };

        // 3. 执行请求（失败落条目，计时取失败前实际耗时）
        let res = match self.client.execute(req).await {
            Ok(res) => res,
            Err(e) => {
                return Self::transport_error_entry(
                    entry,
                    request_number,
                    target_url,
                    start.elapsed(),
                    &e,
                );
            }
        };
        let elapsed = start.elapsed();

        // 4. 比对状态码（精确相等语义，body diff 属 T4 Smart Diff）
        let expected_status = entry.response.status;
        let actual_status = res.status.code();
        let status_matched = expected_status == actual_status;

        if status_matched {
            println!(
                "  {} Status matched: {} (in {:.2?})",
                "[✓]".bold().green(),
                actual_status.to_string().green(),
                elapsed
            );
        } else {
            println!(
                "  {} Status mismatch: expected {}, got {}",
                "[✗]".bold().red(),
                expected_status.to_string().green(),
                actual_status.to_string().red()
            );
        }

        if self.verbose {
            println!("  ┌─ Replayed Response Body ──────────────────────────");
            println!("  │ {}", res.body);
            println!("  └──────────────────────────────────────────────────");
        }

        ReplayEntryResult {
            request_number,
            method: entry.request.method.clone(),
            url: target_url,
            expected_status,
            actual_status: Some(actual_status),
            matched: status_matched,
            duration_ms: elapsed.as_millis() as u64,
            error: None,
        }
    }

    /// 从快照条目还原请求（URL 解析/header 还原失败以 Err 上浮，由调用方落条目）
    fn restore_request(entry: &SnapshotEntry, target_url: &str) -> Result<Request> {
        let mut req = Request::new(&entry.request.method, target_url)?;
        for (name, val) in &entry.request.headers {
            if let Ok(val_str) = val.to_str() {
                req = req.with_header(name.as_str(), val_str)?;
            }
        }
        if let Some(body_str) = &entry.request.body {
            req = req.with_body(body_str);
        }
        // 还原 @timeout（旧快照无此字段时为 None，走 client 级默认）
        req = req.with_timeout(entry.request.timeout);
        Ok(req)
    }

    /// 传输层失败条目：`actual_status=None` + `error=Some(首行)`，计入 failed。
    /// duration 取请求发起至失败的实际耗时（禁止硬编码 0）。
    fn transport_error_entry(
        entry: &SnapshotEntry,
        request_number: usize,
        url: String,
        elapsed: Duration,
        error: &impl std::fmt::Display,
    ) -> ReplayEntryResult {
        let message = error
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        println!("  {} Replay error: {}", "[✗]".bold().red(), message);
        ReplayEntryResult {
            request_number,
            method: entry.request.method.clone(),
            url,
            expected_status: entry.response.status,
            actual_status: None,
            matched: false,
            duration_ms: elapsed.as_millis() as u64,
            error: Some(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(request_number: usize) -> ReplayEntryResult {
        ReplayEntryResult {
            request_number,
            method: "GET".to_string(),
            url: "http://127.0.0.1:1/path?token=secret".to_string(),
            expected_status: 200,
            actual_status: Some(404),
            matched: false,
            duration_ms: 42,
            error: None,
        }
    }

    #[test]
    fn test_replay_entry_result_serde_round_trip() {
        let entry = sample_entry(2);
        let json = serde_json::to_string(&entry).unwrap();
        let back: ReplayEntryResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.request_number, 2);
        assert_eq!(back.expected_status, 200);
        assert_eq!(back.actual_status, Some(404));
        assert!(!back.matched);
        assert_eq!(back.duration_ms, 42);
        assert!(back.error.is_none());
    }

    #[test]
    fn test_replay_entry_result_transport_error_round_trip() {
        let mut entry = sample_entry(3);
        entry.actual_status = None;
        entry.error = Some("error sending request".to_string());
        let json = serde_json::to_string(&entry).unwrap();
        let back: ReplayEntryResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.actual_status, None);
        assert_eq!(back.error.as_deref(), Some("error sending request"));
    }

    #[test]
    fn test_replay_entry_result_missing_optional_fields_backfill() {
        // 旧形态 JSON 缺省 Option 字段时经 #[serde(default)] 反序列化为 None
        let back: ReplayEntryResult = serde_json::from_str(
            r#"{"request_number":1,"method":"GET","url":"http://x/","expected_status":200,"matched":true,"duration_ms":5}"#,
        )
        .unwrap();
        assert_eq!(back.actual_status, None);
        assert_eq!(back.error, None);
    }
}
