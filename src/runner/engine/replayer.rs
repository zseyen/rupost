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
    /// 构造期一次性解析的 target（审查 P0-B：target 非法在 new 即 Err 快失败，
    /// 杜绝 per-entry `if let Ok` 静默跳过覆写——流量会打向快照原始主机）
    parsed_target: Option<url::Url>,
    verbose: bool,
    /// T4 追加排除的叶子键名（默认列表之外，大小写敏感精确匹配）
    ignore_fields: Vec<String>,
    /// T4 逃生舱：true 时停用 body 比对，恢复 R6 仅状态码语义
    status_only: bool,
}

/// body 比对参与上限：任一侧超过即跳过（body_matched=None），防 MB 级全量 parse
const BODY_COMPARE_MAX_BYTES: usize = 256 * 1024;
/// 默认排除列表（14 键，叶子键名精确匹配、大小写敏感；剔除裸 date/time 高业务误伤键）
const DEFAULT_IGNORE_FIELDS: [&str; 14] = [
    "timestamp",
    "created_at",
    "updated_at",
    "uuid",
    "request_id",
    "requestId",
    "trace_id",
    "traceId",
    "span_id",
    "token",
    "session_id",
    "sessionId",
    "access_token",
    "refresh_token",
];

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
    /// body 比对结论：None=未参与比对（占位 body/超限/--status-only/状态码不等短路）
    #[serde(default)]
    pub body_matched: Option<bool>,
    /// body 差异脱敏摘要（前 3 处 `path: expected → actual`，512B 截断）；仅 body 失败时非空
    #[serde(default)]
    pub body_diff: Option<String>,
    /// 实际响应片段（采集点经 redact_and_truncate_body 脱敏）；仅失败条目采集，供 AI 桥
    #[serde(default)]
    pub response_body_snippet: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReplayReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    #[serde(default)]
    pub entries: Vec<ReplayEntryResult>,
}

impl ReplayExecutor {
    pub fn new(target_url: Option<String>, verbose: bool) -> crate::Result<Self> {
        let parsed_target = match target_url.as_deref() {
            // 无 scheme 的裸 host（如 `--target example.com`）是最高频误写形态，
            // 此前静默跳过全部覆写 → 重放流量打向快照原始主机
            Some(t) => Some(url::Url::parse(t).map_err(|e| {
                crate::error::RupostError::ParseError(format!(
                    "--target '{t}' 不是合法 URL（需含 scheme，如 http://host:port）: {e}"
                ))
            })?),
            None => None,
        };
        Ok(Self {
            client: Client::new(None),
            target_url,
            parsed_target,
            verbose,
            ignore_fields: Vec::new(),
            status_only: false,
        })
    }

    /// 追加排除键（`--ignore-fields`，逗号分隔）
    pub fn with_ignore_fields(mut self, keys: Vec<String>) -> Self {
        self.ignore_fields.extend(keys);
        self
    }

    /// 停用 body 比对（`--status-only`，恢复 R6 仅状态码语义）
    pub fn status_only(mut self) -> Self {
        self.status_only = true;
        self
    }

    /// 执行快照重放并输出对比结果
    pub async fn replay_file<P: AsRef<Path>>(&self, path: P) -> Result<ReplayReport> {
        let suite = crate::history::read_snapshot_suite_compat(path)?;
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
                    (None, Some(_actual)) if e.body_diff.is_some() => {
                        // T4 第四形态：状态匹配但 body 失败（R6 §4.2 形态表扩充行）
                        println!(
                            "   - #{} {} {} (body: {})",
                            e.request_number,
                            e.method,
                            e.url,
                            e.body_diff.as_deref().unwrap_or_default()
                        );
                    }
                    (None, Some(actual)) => {
                        println!(
                            "   - #{} {} {} (expected {}, got {})",
                            e.request_number, e.method, e.url, e.expected_status, actual
                        );
                    }
                    (None, None) => {
                        // 防御臂：进程内不可达（matched=false 蕴含 error 或 actual 至少一者存在），
                        // 仅未来形态扩展时兜底，不暗示存在第三归类
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
        if let Some(parsed_target) = &self.parsed_target
            && let Ok(mut parsed_url) = url::Url::parse(&entry.request.url)
        {
            // 构造期已保证 target 合法；set_* 的 .ok() 仅防御奇异 scheme 转换
            // （逐 entry 确定性相同，告警会刷屏——登记注释即可）
            parsed_url.set_scheme(parsed_target.scheme()).ok();
            parsed_url.set_host(parsed_target.host_str()).ok();
            parsed_url.set_port(parsed_target.port()).ok();

            // 若 target 显式包含非根路径前缀（如 http://host/api/v1），与原路径组合
            let target_path = parsed_target.path().trim_end_matches('/');
            if !target_path.is_empty() {
                let original_path = parsed_url.path();
                let combined_path = crate::http::url::join_paths(&[target_path, original_path]);
                parsed_url.set_path(&combined_path);
            }

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
                    false,
                );
            }
        };

        // 3. 执行请求（失败落条目，计时取失败前实际耗时）
        let res = match self.client.execute(req).await {
            Ok(res) => res,
            Err(e) => {
                let is_expected_failure = entry.response.is_none();
                return Self::transport_error_entry(
                    entry,
                    request_number,
                    target_url,
                    start.elapsed(),
                    &e,
                    is_expected_failure,
                );
            }
        };
        let elapsed = start.elapsed();

        // 4. 比对状态码（若快照记录的是传输层网络故障，收到响应则视为行为不一致）
        let Some(ref recorded_resp) = entry.response else {
            let err_desc = entry
                .error
                .as_ref()
                .map(|e| e.message.as_str())
                .unwrap_or("transport failure");
            let mismatch_msg = format!(
                "Snapshot recorded transport error ({err_desc}), but replay received HTTP {}",
                res.status.code()
            );
            println!(
                "  {} [✗] Unexpected response: {}",
                "[✗]".bold().red(),
                mismatch_msg
            );
            return ReplayEntryResult {
                request_number,
                method: entry.request.method.clone(),
                url: target_url,
                expected_status: 0,
                actual_status: Some(res.status.code()),
                matched: false,
                duration_ms: elapsed.as_millis() as u64,
                error: Some(mismatch_msg),
                body_matched: Some(false),
                body_diff: None,
                response_body_snippet: Some(res.body.chars().take(200).collect()),
            };
        };

        let expected_status = recorded_resp.status;
        let actual_status = res.status.code();
        let status_matched = expected_status == actual_status;

        // 5. T4 Smart Diff：body 深比较（默认排除列表 + 用户 --ignore-fields 合并）；
        // 仅状态码相等且未启用 --status-only 时参与
        let (body_matched, body_diff) = if status_matched && !self.status_only {
            let mut keys: Vec<String> = DEFAULT_IGNORE_FIELDS
                .iter()
                .map(|s| s.to_string())
                .collect();
            keys.extend(self.ignore_fields.iter().cloned());
            Self::compare_bodies(&recorded_resp.body, &res.body, &keys)
        } else {
            (None, None)
        };
        let matched = status_matched && body_matched.unwrap_or(true);

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
        // T4 第四形态内联呈现：状态匹配但 body 失败（差异摘要已脱敏）
        if let Some(diff) = &body_diff {
            println!("  {} Body mismatch: {}", "[✗]".bold().red(), diff);
        }

        if self.verbose {
            println!("  ┌─ Replayed Response Body ──────────────────────────");
            println!("  │ {}", res.body);
            println!("  └──────────────────────────────────────────────────");
        }

        // 失败条目采集脱敏片段（Gap Review P3：直上 LLM 前必经脱敏截断）
        let response_body_snippet = if matched {
            None
        } else {
            Some(crate::ai::redactor::redact_and_truncate_body(
                &res.body,
                crate::ai::redactor::DEFAULT_SNIPPET_MAX_BYTES,
            ))
        };

        ReplayEntryResult {
            request_number,
            method: entry.request.method.clone(),
            url: target_url,
            expected_status,
            actual_status: Some(actual_status),
            matched,
            duration_ms: elapsed.as_millis() as u64,
            error: None,
            body_matched,
            body_diff,
            response_body_snippet,
        }
    }

    /// T4 body 比对：返回 (结论, 差异摘要)。
    /// - 快照 body 为占位标记文本或空 → 未参与比对（None, None）——旧导出条目无 body 可言
    /// - 双侧任一超过 256KB → 未参与比对
    /// - 双侧均可 JSON 解析 → 深比较（排除键双侧剔除）
    /// - 解析能力不对等 → body 失败（内容类型漂移）
    /// - 双侧均不可解析 → 原文相等比较（不等报 `body changed (non-JSON)`，关 200→200 换页盲区）
    fn compare_bodies(
        expected_body: &str,
        actual_body: &str,
        ignore_fields: &[String],
    ) -> (Option<bool>, Option<String>) {
        const PLACEHOLDER: &str = "Old snapshot: body not recorded";
        if expected_body.is_empty() || expected_body == PLACEHOLDER {
            return (None, None);
        }
        if expected_body.len() > BODY_COMPARE_MAX_BYTES
            || actual_body.len() > BODY_COMPARE_MAX_BYTES
        {
            return (None, None);
        }

        let expected_json = serde_json::from_str::<serde_json::Value>(expected_body).ok();
        let actual_json = serde_json::from_str::<serde_json::Value>(actual_body).ok();
        match (expected_json, actual_json) {
            (Some(exp), Some(act)) => {
                let mut diffs: Vec<String> = Vec::new();
                let equal = diff_values(&exp, &act, "", ignore_fields, &mut diffs);
                if equal {
                    (Some(true), None)
                } else {
                    let summary = if diffs.is_empty() {
                        "body differs".to_string()
                    } else {
                        diffs.join("; ")
                    };
                    let summary = crate::ai::redactor::redact_free_text(&summary);
                    let summary = crate::security::redact::mask_pii(&summary);
                    let summary = crate::ai::redactor::truncate_utf8_safe(&summary, 512);
                    (Some(false), Some(summary))
                }
            }
            (Some(_), None) | (None, Some(_)) => {
                // 解析能力不对等：一侧 JSON 一侧非 JSON（内容类型漂移）
                (
                    Some(false),
                    Some("body changed (JSON parse mismatch)".to_string()),
                )
            }
            (None, None) => {
                // 双侧均非 JSON：原文比较（关 200→200 HTML 换 JSON 盲区）
                if expected_body == actual_body {
                    (Some(true), None)
                } else {
                    (Some(false), Some("body changed (non-JSON)".to_string()))
                }
            }
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

    /// 传输层失败条目：若快照原本记录的就是网络异常则判定吻合（matched=true），否则计入 failed。
    /// duration 取请求发起至失败的实际耗时（禁止硬编码 0）。
    fn transport_error_entry(
        entry: &SnapshotEntry,
        request_number: usize,
        url: String,
        elapsed: Duration,
        error: &impl std::fmt::Display,
        matched_expected_failure: bool,
    ) -> ReplayEntryResult {
        let message = error
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        if matched_expected_failure {
            println!(
                "  {} [✓] Transport failure matched snapshot ({})",
                "[✓]".bold().green(),
                message
            );
        } else {
            println!("  {} Replay error: {}", "[✗]".bold().red(), message);
        }
        ReplayEntryResult {
            request_number,
            method: entry.request.method.clone(),
            url,
            expected_status: entry.response.as_ref().map(|r| r.status).unwrap_or(0),
            actual_status: None,
            matched: matched_expected_failure,
            duration_ms: elapsed.as_millis() as u64,
            error: if matched_expected_failure {
                None
            } else {
                Some(message)
            },
            body_matched: None,
            body_diff: None,
            response_body_snippet: None,
        }
    }
}

/// T4 深比较：递归比较两个 JSON 值，跳过排除键（双侧剔除、大小写敏感精确匹配），
/// 收集前 3 处差异（`path: expected → actual`，path 记法 `users[2].id`）。
/// 语义定案（PRD R5）：数组序敏感；int/float 判不等；顶层标量/标量数组/数组长度变化
/// 无键上下文 = 不可排除 = 真实差异。
fn diff_values(
    expected: &serde_json::Value,
    actual: &serde_json::Value,
    path: &str,
    ignore: &[String],
    diffs: &mut Vec<String>,
) -> bool {
    match (expected, actual) {
        (serde_json::Value::Object(exp), serde_json::Value::Object(act)) => {
            let mut equal = true;
            for (key, exp_val) in exp {
                if ignore.iter().any(|k| k == key) {
                    continue;
                }
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match act.get(key) {
                    None => {
                        let v = mask_by_key(&child, &short_value(exp_val));
                        push_diff(diffs, &mut equal, &child, &v, "<missing>");
                    }
                    Some(act_val) => {
                        if !diff_values(exp_val, act_val, &child, ignore, diffs) {
                            equal = false;
                        }
                    }
                }
                if diffs.len() >= 3 {
                    return false;
                }
            }
            for key in act.keys() {
                if ignore.iter().any(|k| k == key) {
                    continue;
                }
                if !exp.contains_key(key) {
                    let child = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    let v = mask_by_key(&child, &short_value(&act[key]));
                    push_diff(diffs, &mut equal, &child, "<missing>", &v);
                    if diffs.len() >= 3 {
                        return false;
                    }
                }
            }
            equal
        }
        (serde_json::Value::Array(exp), serde_json::Value::Array(act)) => {
            if exp.len() != act.len() {
                push_diff(
                    diffs,
                    &mut false,
                    &format!("{path}(len)"),
                    &exp.len().to_string(),
                    &act.len().to_string(),
                );
                return false;
            }
            let mut equal = true;
            for (i, (exp_val, act_val)) in exp.iter().zip(act.iter()).enumerate() {
                let child = format!("{path}[{i}]");
                if !diff_values(exp_val, act_val, &child, ignore, diffs) {
                    equal = false;
                }
                if diffs.len() >= 3 {
                    return false;
                }
            }
            equal
        }
        _ => {
            // 标量（或双侧类型不同）：直接相等比较（序敏感/大小写敏感/int≠float 均为 serde_json 原生语义）
            if expected == actual {
                true
            } else {
                let label = if path.is_empty() { "(root)" } else { path };
                // 敏感键上的值差异整体 [REDACTED]（对齐 redact_assertion_value 约定，值不外泄）
                let exp_disp = mask_by_key(label, &short_value(expected));
                let act_disp = mask_by_key(label, &short_value(actual));
                push_diff(diffs, &mut false, label, &exp_disp, &act_disp);
                false
            }
        }
    }
}

/// 按叶子键名掩码差异值：路径命中敏感键（token/authorization 等）→ 值整体 `[REDACTED]`。
/// 复用 `assertion_targets_sensitive_key`（分词匹配，`orders[0].access_token` 命中）。
fn mask_by_key(path: &str, value: &str) -> String {
    if crate::ai::redactor::assertion_targets_sensitive_key(path) {
        "[REDACTED]".to_string()
    } else {
        value.to_string()
    }
}

/// 记录一处差异并置不等旗标
fn push_diff(
    diffs: &mut Vec<String>,
    equal_flag: &mut bool,
    path: &str,
    expected: &str,
    actual: &str,
) {
    diffs.push(format!("{path}: {expected} → {actual}"));
    *equal_flag = false;
}

/// 标量差异的单侧展示截断（40 字符，防超长字符串撑爆摘要）
fn short_value(v: &serde_json::Value) -> String {
    crate::utils::text::truncate_chars(&v.to_string(), 40, "…")
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
            body_matched: None,
            body_diff: None,
            response_body_snippet: None,
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
        assert_eq!(back.body_matched, None);
        assert_eq!(back.body_diff, None);
        assert_eq!(back.response_body_snippet, None);
    }

    fn diff_of(exp: &str, act: &str, ignore: &[&str]) -> (Option<bool>, Option<String>) {
        let keys: Vec<String> = ignore.iter().map(|s| s.to_string()).collect();
        ReplayExecutor::compare_bodies(exp, act, &keys)
    }

    /// T4 决策表：抖动键双侧剔除后相等；真实差异被抓；排除键上的回归被掩盖（明示假通过面）
    #[test]
    fn test_compare_bodies_jitter_and_real_diff() {
        // 抖动容忍：timestamp 值不同但被默认语义排除（测试经调用方传键）
        let (m, d) = diff_of(r#"{"v":1,"ts":"a"}"#, r#"{"v":1,"ts":"b"}"#, &["ts"]);
        assert_eq!(m, Some(true));
        assert_eq!(d, None);
        // 真实差异被抓：业务字段变化
        let (m, d) = diff_of(r#"{"quota":100}"#, r#"{"quota":200}"#, &[]);
        assert_eq!(m, Some(false));
        let diff = d.unwrap();
        assert!(diff.contains("quota"), "diff names the path: {diff}");
        assert!(
            diff.contains("100") && diff.contains("200"),
            "diff shows both: {diff}"
        );
        // 嵌套路径记法
        let (m, d) = diff_of(r#"{"user":{"id":7}}"#, r#"{"user":{"id":8}}"#, &[]);
        assert_eq!(m, Some(false));
        let diff = d.unwrap();
        assert!(diff.contains("user.id"), "nested path notation: {diff}");
        // 数组内对象 + 下标记法
        let (m, d) = diff_of(r#"{"orders":[{"id":1}]}"#, r#"{"orders":[{"id":9}]}"#, &[]);
        assert_eq!(m, Some(false));
        let diff = d.unwrap();
        assert!(
            diff.contains("orders[0].id"),
            "array index notation: {diff}"
        );
    }

    /// T4 决策表：无键上下文形态（顶层标量/标量数组/数组长度）= 不可排除 = 真实差异
    #[test]
    fn test_compare_bodies_keyless_forms() {
        // 顶层标量
        let (m, _) = diff_of(r#""abc""#, r#""xyz""#, &["abc", "xyz"]);
        assert_eq!(m, Some(false), "top-level scalar has no key context");
        // 标量数组（长度同、值异）
        let (m, _) = diff_of("[1,2,3]", "[1,2,4]", &["4"]);
        assert_eq!(m, Some(false), "scalar arrays are order/value sensitive");
        // 数组长度变化
        let (m, d) = diff_of(r#"{"l":[1]}"#, r#"{"l":[1,2]}"#, &[]);
        assert_eq!(m, Some(false));
        let diff = d.unwrap();
        assert!(diff.contains("len"), "length change reported: {diff}");
    }

    /// T4 决策表：序敏感与 int≠float（serde_json 原生语义，PRD R5 已知限制）
    #[test]
    fn test_compare_bodies_order_and_number_semantics() {
        let (m, _) = diff_of(r#"{"a":[1,2]}"#, r#"{"a":[2,1]}"#, &[]);
        assert_eq!(m, Some(false), "array order sensitive by design");
        let (m, _) = diff_of(r#"{"v":1}"#, r#"{"v":1.0}"#, &[]);
        assert_eq!(m, Some(false), "int vs float unequal by design");
    }

    /// 变异②守护：前 3 处截断——第 4 处差异不进摘要；512B 截断分支真实触发
    #[test]
    fn test_compare_bodies_three_cap_and_truncation() {
        let exp = r#"{"a":1,"b":2,"c":3,"d":4}"#;
        let act = r#"{"a":9,"b":9,"c":9,"d":9}"#;
        let (m, d) = diff_of(exp, act, &[]);
        assert_eq!(m, Some(false));
        let diff = d.unwrap();
        assert!(diff.contains("a: 1"), "first diff present: {diff}");
        assert!(diff.contains("c: 3"), "third diff present: {diff}");
        assert!(
            !diff.contains("d: 4"),
            "fourth diff capped by 3-limit: {diff}"
        );
        // 512B 截断分支：3 处差异 × 超长路径名（short_value 只裁值不裁路径，摘要必超限）
        let key = "K".repeat(180);
        let long_exp = format!(r#"{{"{key}":1,"{key}2":2,"{key}3":3}}"#);
        let long_act = format!(r#"{{"{key}":9,"{key}2":9,"{key}3":9}}"#);
        let (m, d) = diff_of(&long_exp, &long_act, &[]);
        assert_eq!(m, Some(false));
        let diff = d.unwrap();
        assert!(
            diff.contains("...[truncated]"),
            "512B truncation must fire: len={} {diff}",
            diff.len()
        );
    }

    /// 变异⑦守护扩展：DEFAULT_IGNORE_FIELDS 14 键逐一过匹配器——常量拼写/大小写
    /// 回归即红（compare_bodies 不内含 DEFAULT 合并，单测显式传常量锁机制+常量链路）
    #[test]
    fn test_default_ignore_fields_all_guarded() {
        for key in DEFAULT_IGNORE_FIELDS {
            let exp = format!(r#"{{"{}":"a"}}"#, key);
            let act = format!(r#"{{"{}":"b"}}"#, key);
            let (m, d) = diff_of(&exp, &act, &DEFAULT_IGNORE_FIELDS);
            assert_eq!(
                m,
                Some(true),
                "default key {key:?} must be excluded (jitter tolerated)"
            );
            assert!(d.is_none(), "excluded key {key:?} leaves no diff");
        }
    }

    /// T4 skip 分支：占位文本/空 body/超限 → 未参与比对；解析能力不对等 → 内容类型漂移；
    /// 双侧非 JSON 原文不等 → body changed (non-JSON)
    #[test]
    fn test_compare_bodies_skip_and_non_json() {
        // 占位文本跳过（旧导出条目无 body）
        let (m, d) = diff_of("Old snapshot: body not recorded", "anything", &[]);
        assert_eq!((m, d), (None, None));
        // 空 body 跳过
        let (m, d) = diff_of("", "anything", &[]);
        assert_eq!((m, d), (None, None));
        // 超限跳过
        let big = "x".repeat(BODY_COMPARE_MAX_BYTES + 1);
        let (m, d) = diff_of(&big, "small", &[]);
        assert_eq!((m, d), (None, None));
        // JSON vs 非 JSON：内容类型漂移
        let (m, d) = diff_of(r#"{"a":1}"#, "plain text", &[]);
        assert_eq!(m, Some(false));
        assert!(d.unwrap().contains("JSON parse mismatch"));
        // 双侧非 JSON 原文相等 → 通过
        let (m, _) = diff_of("hello", "hello", &[]);
        assert_eq!(m, Some(true));
        // 双侧非 JSON 原文不等 → 200→200 换页盲区关闭
        let (m, d) = diff_of("<html>old</html>", "<html>new</html>", &[]);
        assert_eq!(m, Some(false));
        assert!(d.unwrap().contains("body changed (non-JSON)"));
    }

    /// T4 安全不变量：body_diff 摘要经脱敏（token 值不外泄）+ 512B 截断
    #[test]
    fn test_compare_bodies_diff_redacted_and_capped() {
        let (m, d) = diff_of(
            r#"{"token":"super-secret-val","k":"aaa"}"#,
            r#"{"token":"leaked-other","k":"bbb"}"#,
            &[],
        );
        assert_eq!(m, Some(false));
        let diff = d.unwrap();
        assert!(
            !diff.contains("super-secret-val") && !diff.contains("leaked-other"),
            "diff values must be redacted: {diff}"
        );
        assert!(
            diff.len() <= 512 + "...[truncated]".len(),
            "capped at 512B: {}",
            diff.len()
        );
    }
}
