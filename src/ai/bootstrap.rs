//! F8 `ai bootstrap --from history`：调试历史 → 语义增强回归测试文件。
//!
//! 五阶段管线（B1 增强型）：A 采集（确定性）→ B 机械还原（`HttpGenerator`，独立可用）
//! → C AI 语义增强（可失败可跳过）→ D 引擎裁决（断言 parser + capture 语法 lint）
//! → E 原子落盘（防覆盖）。AI 缺失/故障时降级为机械基线 + 黄色警告，exit 0。

use crate::ai::config;
use crate::ai::redactor::{redact_and_truncate_body, redact_url};
use crate::assertion::parse_assertion;
use crate::history::storage::HistoryStorage;
use crate::{Result, RupostError};
use chrono::Utc;
use colored::Colorize;
use rupost_ai::{SynthesisAdvice, SynthesisItem, SynthesisOutcome, TestSynthesizer};

/// `--last` 上限（防单次 prompt 体量失控；超出报错建议 `--filter`）
pub const MAX_LAST: usize = 50;
/// 每块 AI 建议断言数上限（lint 层截断）
pub const MAX_ASSERTIONS_PER_BLOCK: usize = 8;

/// bootstrap 运行参数
#[derive(Debug, Clone, Default)]
pub struct BootstrapParams {
    /// 素材来源（MVP 仅支持 "history"）
    pub from: String,
    /// 取最近 N 条历史
    pub last: usize,
    /// URL 包含过滤（contains 子串匹配；R8-1：素材跨会话混杂时收窄）
    pub filter: Option<String>,
    /// 输出文件（防覆盖）
    pub output: String,
    /// 跳过 C 阶段（显式纯机械基线）
    pub no_ai: bool,
    /// origin 模板化为 `{{base_url}}`（透传发射器）
    pub templatize_host: bool,
}

/// D 阶段裁决：断言过 parser、截断至 8 条；非法条目返回 (丢弃数, 合法断言行)
fn lint_assertions(index: usize, assertions: &[String]) -> (usize, Vec<String>) {
    let mut ok = Vec::new();
    let mut dropped = 0usize;
    for a in assertions {
        let trimmed = a.trim();
        if trimmed.is_empty() {
            continue;
        }
        if parse_assertion(trimmed).is_ok() && ok.len() < MAX_ASSERTIONS_PER_BLOCK {
            ok.push(format!("# @assert {trimmed}"));
        } else {
            dropped += 1;
        }
    }
    let _ = index;
    (dropped, ok)
}

/// D 阶段裁决：capture 行语法校验（`<name> from body.<path>` / `<name> from header.<name>`）
fn lint_capture(capture: &str) -> std::result::Result<String, String> {
    let (name, source) = capture
        .trim()
        .split_once(" from ")
        .ok_or_else(|| format!("capture 缺少 ' from ': {capture}"))?;
    let name_ok =
        !name.trim().is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let source_ok = source.starts_with("body.") || source.starts_with("header.");
    if !name_ok || !source_ok {
        return Err(format!("capture 语法非法: {capture}"));
    }
    Ok(format!("# @capture {} from {}", name.trim(), source.trim()))
}

/// 素材条目 → 发送给 LLM 的脱敏 DTO（url 也掩码——素材只喂认知，不供回放）
fn synthesis_item(index: usize, entry: &crate::history::model::HistoryEntry) -> SynthesisItem {
    SynthesisItem {
        index,
        method: entry.request.method.clone(),
        url: redact_url(&entry.request.url),
        status_code: Some(entry.response.status),
        request_body_snippet: entry
            .request
            .body
            .as_deref()
            .map(|b| redact_and_truncate_body(b, 4096))
            .filter(|s| !s.is_empty()),
        response_body_snippet: entry
            .response
            .body
            .as_deref()
            .map(|b| redact_and_truncate_body(b, 4096))
            .filter(|s| !s.is_empty()),
        available_names: vec![],
    }
}

/// B 阶段（机械还原）+ D 阶段（AI 建议合并）：逐条发射块，AI 断言经 lint 后追加到
/// 该块隐式 status 断言之后；captures 同样 lint 后追加。
fn emit_blocks(
    // 固有限制：按 "### " 前缀切块对齐 advice 索引——非 JSON 多行 body 含
    // "### " 形态标题时会错位（.http 格式固有限制；generator 产出的机械基线不受影响）
    entries: &[crate::history::model::HistoryEntry],
    outcome: Option<&SynthesisOutcome>,
    templatize_host: bool,
) -> (String, usize, usize) {
    let advice_by_index: std::collections::HashMap<usize, &SynthesisAdvice> = outcome
        .map(|o| {
            o.advice
                .iter()
                .map(|a| (a.index, a))
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default();

    let mut out = String::new();
    let mut enhanced = 0usize;
    let mut lint_dropped = 0usize;

    // ── 批量发射一次（Review P1-1：跨块共享 @name 去重表与变量头，禁止逐条调用） ──
    let (full, _) =
        crate::generator::HttpGenerator::generate_with_variables(entries, templatize_host)
            .unwrap_or_default();

    // 按块边界（"### " 行）切分：前导注释（变量声明头）单独保存，
    // 块与条目一一对应（advice index 直接对位，无偏移）
    let mut preamble = String::new();
    let mut blocks: Vec<Vec<String>> = Vec::new();
    for line in full.lines() {
        if line.starts_with("### ") {
            blocks.push(vec![line.to_string()]);
            continue;
        }
        match blocks.last_mut() {
            Some(b) => b.push(line.to_string()),
            None => preamble.push_str(&format!("{line}\n")),
        }
    }

    for (block_idx, block_lines) in blocks.iter_mut().enumerate() {
        if let Some(advice) = advice_by_index.get(&block_idx) {
            let (dropped, assert_lines) = lint_assertions(block_idx, &advice.assertions);
            lint_dropped += dropped;
            let mut extra = String::new();
            for cap in &advice.captures {
                match lint_capture(cap) {
                    Ok(line) => extra.push_str(&format!("{line}\n")),
                    Err(_) => lint_dropped += 1,
                }
            }
            if !extra.is_empty() {
                // capture 插在首个 status 断言之前（块尾元数据区）
                if let Some(pos) = block_lines.iter().position(|l| l.starts_with("# @assert ")) {
                    for (off, cap_line) in extra.lines().enumerate() {
                        block_lines.insert(pos + off, cap_line.to_string());
                    }
                } else {
                    for cap_line in extra.lines() {
                        block_lines.push(cap_line.to_string());
                    }
                }
            }
            if !assert_lines.is_empty() {
                enhanced += 1;
                for l in &assert_lines {
                    block_lines.push(l.clone());
                }
                // depends_note 为预留通道：当前 prompt schema 未请求该字段，
                // AI 返回时才生效（serde default None）
                if let Some(note) = &advice.depends_note {
                    block_lines.push(format!("# 建议依赖: {note}"));
                }
            }
        }
    }

    if !preamble.is_empty() {
        out.push_str(preamble.trim_end());
        out.push_str("\n\n");
    }
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&block.join("\n"));
    }
    out.push('\n');

    if lint_dropped > 0 {
        eprintln!(
            "{} {} 条 AI 建议未通过本地裁决（断言 parser / capture 语法），已丢弃",
            "⚠".yellow(),
            lint_dropped
        );
    }
    (out, enhanced, lint_dropped)
}

/// F8 入口：bootstrap 管线（A 采集 → B 机械还原 → C AI 增强 → D 裁决 → E 落盘）
pub async fn run_bootstrap(params: &BootstrapParams) -> Result<()> {
    if params.from != "history" {
        return Err(RupostError::Other(format!(
            "暂不支持素材来源 '{}': MVP 仅支持 --from history",
            params.from
        )));
    }
    if params.last == 0 || params.last > MAX_LAST {
        return Err(RupostError::Other(format!(
            "--last 需在 1..={}（当前 {}）；素材过多请配合 --filter 收窄",
            MAX_LAST, params.last
        )));
    }

    // ── A 采集 ──
    let entries = HistoryStorage::new()
        .tail(params.last)
        .map_err(|e| RupostError::Other(format!("读取历史失败: {e}")))?;
    let entries: Vec<_> = match &params.filter {
        Some(prefix) => entries
            .into_iter()
            .filter(|e| e.request.url.contains(prefix.as_str()))
            .collect(),
        None => entries,
    };
    if entries.is_empty() {
        return Err(RupostError::Other(
            "暂无符合条件的历史记录：请先执行请求，或放宽 --filter/--last 条件".to_string(),
        ));
    }

    // 防覆盖检查前置（Review P2-6）：AI 合成消耗 token，拒绝应发生在任何调用之前
    let out_path = std::path::Path::new(&params.output);
    if out_path.exists() {
        return Err(RupostError::Other(format!(
            "输出文件 {} 已存在——bootstrap 不覆盖既有文件（AI 产物需人工审查）",
            params.output
        )));
    }

    // ── B 机械还原（确定性基线；AI 路径由 emit_blocks 批量发射——单次调用共享 @name 去重与变量头） ──
    let (baseline, _) =
        crate::generator::HttpGenerator::generate_with_variables(&entries, params.templatize_host)?;

    // ── C AI 语义增强（可跳过/可失败） ──
    let (mut content, enhanced, failed_chunks) = if params.no_ai {
        eprintln!("{} --no-ai：已跳过 AI 增强，输出机械还原基线", "⚠".yellow());
        (baseline, 0usize, 0usize)
    } else {
        let items: Vec<SynthesisItem> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| synthesis_item(i, e))
            .collect();
        let resolved = config::resolve().await.map_err(RupostError::Other)?;
        if let Some(notice) = &resolved.notice {
            // 同 triage：notice 走 stderr 保持 stdout 纯净
            eprintln!("{} {}", "ℹ".cyan(), notice);
        }
        let synthesizer = TestSynthesizer::new(resolved.client.as_ref());
        match synthesizer.synthesize_batch(&items).await {
            Ok(outcome) => {
                if outcome.failed_chunks > 0 {
                    eprintln!(
                        "{} {} 个分块合成失败，对应条目保留机械基线",
                        "⚠".yellow(),
                        outcome.failed_chunks
                    );
                }
                emit_blocks(&entries, Some(&outcome), params.templatize_host)
            }
            Err(e) => {
                eprintln!(
                    "{} AI 合成失败（{}），已降级为机械还原基线",
                    "⚠".yellow(),
                    truncate_msg(&e.to_string(), 120)
                );
                emit_blocks(&entries, None, params.templatize_host)
            }
        }
    };

    // R8-6 来源审计头（变量声明头已由 emit_blocks 从批量发射中带出，不再重复前置）
    content = format!(
        "# generated by rupost ai bootstrap at {} from history(last {})\n\n{}",
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        entries.len(),
        content
    );

    // ── E 落盘 ──
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, &content)?;

    println!(
        "{} 已生成 {}：{} 个请求块（AI 增强 {} 块）",
        "[✓]".green().bold(),
        params.output,
        entries.len(),
        enhanced
    );
    if failed_chunks > 0 {
        eprintln!(
            "{} {failed_chunks} 个分块合成失败（对应条目为机械基线）",
            "⚠".yellow()
        );
    }
    Ok(())
}

fn truncate_msg(msg: &str, max: usize) -> String {
    if msg.chars().count() <= max {
        msg.to_string()
    } else {
        format!("{}…", msg.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::{RequestSnapshot, ResponseMeta};
    use chrono::Utc;
    use reqwest::header::HeaderMap;

    fn entry(num: usize, url: &str) -> crate::history::model::HistoryEntry {
        crate::history::model::HistoryEntry {
            id: format!("id-{num}"),
            timestamp: Utc::now(),
            duration_ms: 1,
            request: RequestSnapshot {
                method: "GET".to_string(),
                url: url.to_string(),
                headers: HeaderMap::new(),
                body: None,
                timeout: None,
            },
            source: Some("test".to_string()),
            response: ResponseMeta {
                status: 200,
                headers: HeaderMap::new(),
                body: Some(r#"{"token":"[REDACTED]"}"#.to_string()),
            },
        }
    }

    #[test]
    fn test_lint_assertions_filters_invalid_and_caps() {
        let advice = vec![
            "status == 200".to_string(),
            "body.token exists".to_string(),
            "not an assertion".to_string(), // parser 拒绝
            "body.x ~= 1".to_string(),      // 非法运算符
        ];
        let (dropped, ok) = lint_assertions(0, &advice);
        assert_eq!(dropped, 2);
        assert_eq!(ok.len(), 2);
        assert!(ok[0].starts_with("# @assert status == 200"));
    }

    #[test]
    fn test_lint_assertions_caps_at_max() {
        let advice: Vec<String> = (0..12).map(|i| format!("status != {i}")).collect();
        let (dropped, ok) = lint_assertions(0, &advice);
        assert_eq!(ok.len(), MAX_ASSERTIONS_PER_BLOCK);
        assert_eq!(dropped, 4);
    }

    #[test]
    fn test_lint_capture_syntax() {
        assert!(lint_capture("token from body.token").is_ok());
        assert!(lint_capture("server_date from header.date").is_ok());
        assert!(lint_capture("bad syntax here").is_err());
        assert!(lint_capture("name from query.x").is_err());
    }

    #[test]
    fn test_emit_blocks_merges_advice_after_status_assert() {
        let entries = vec![entry(1, "http://a.local/v1/login")];
        let _baseline = crate::generator::HttpGenerator::generate_with_variables(&entries, false)
            .unwrap()
            .0;
        // 用 baseline 反向构造 advice 场景：直接校验 emit 合并逻辑
        let advice = SynthesisAdvice {
            index: 0,
            assertions: vec!["body.token exists".to_string()],
            captures: vec![],
            depends_note: None,
        };
        let outcome = SynthesisOutcome {
            advice: vec![advice],
            failed_chunks: 0,
        };
        let (text, enhanced, dropped) = emit_blocks(&entries, Some(&outcome), false);
        assert_eq!(enhanced, 1);
        assert_eq!(dropped, 0);
        assert!(text.contains("# @assert body.token exists"));
        assert!(text.contains("# @assert status == 200"));
    }

    #[test]
    fn test_emit_blocks_none_is_baseline() {
        let entries = vec![entry(1, "http://a.local/v1/login")];
        let (text, enhanced, dropped) = emit_blocks(&entries, None, false);
        assert_eq!(enhanced, 0);
        assert_eq!(dropped, 0);
        assert!(text.contains("# @assert status == 200"));
        assert!(!text.contains("body.token exists"));
    }
}
