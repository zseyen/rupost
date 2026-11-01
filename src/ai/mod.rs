pub mod bootstrap;
pub mod config;
pub mod context;
pub mod explain;
pub mod redactor;
pub mod resolve;
pub mod rules;
pub mod triage;

use crate::{Result, RupostError};
use colored::Colorize;
use redactor::truncate_chars;
use rupost_ai::AssertionTranslator;

/// `ai assert` 运行参数（P0 收尾：上下文感知 + 脱敏开关 + 零配置链）
#[derive(Debug, Clone, Default)]
pub struct AiAssertParams {
    pub raw: bool,
    /// `--last N`：取最近 N 条历史作为响应上下文
    pub last: Option<usize>,
    /// `--response <FILE>`：显式响应文件上下文（与 last 互斥，clap 层保证）
    pub response: Option<String>,
    /// `--no-mask`：显式关闭上下文脱敏（打印一次性醒目风险提示）
    pub no_mask: bool,
}

/// `rupost ai assert`: 将自然语言期望翻译为标准 @assert 断言行（创作期辅助）。
/// 配置走 B4 链（env > ~/.rupost/ai.toml > 本地探测）；`--last/--response` 提供脱敏响应上下文。
pub async fn run_ai_assert(params: &AiAssertParams, text: &str) -> Result<()> {
    let resolved = crate::ai::config::resolve()
        .await
        .map_err(RupostError::Other)?;
    if let Some(notice) = &resolved.notice {
        // 提示走 stderr：--raw 的 stdout 是机器可读管道，不得混入
        eprintln!("{} {}", "ℹ".cyan(), notice);
    }

    // ── 上下文采集（确定性；错误直接上抛为可读指引）──
    // mask = !no_mask：默认脱敏；--no-mask 时原样保留（仅截断），并打印一次性风险提示
    let mask = !params.no_mask;
    if params.no_mask && (params.response.is_some() || params.last.is_some()) {
        eprintln!(
            "{} --no-mask 已显式关闭脱敏：上下文中的敏感值将原样发送给 AI 端点，请确认合规",
            "⚠".yellow().bold()
        );
    }

    let context = if let Some(path) = &params.response {
        Some(crate::ai::context::response_file_context(path, mask)?)
    } else if let Some(n) = params.last {
        let entries = crate::ai::context::collect_last(n)?;
        Some(crate::ai::context::history_context(&entries, mask))
    } else {
        None
    };

    let user_prompt = crate::ai::context::assemble_user_prompt(text, context);

    let translator = AssertionTranslator::new(resolved.client);
    let assertions = translator
        .translate(&user_prompt)
        .await
        .map_err(|e| RupostError::Other(truncate_chars(&e.to_string(), 500)))?;

    if params.raw {
        for assertion in &assertions {
            println!("# @assert {assertion}");
        }
    } else {
        println!(
            "{} 已生成 {} 条标准断言（粘贴到请求块末尾即可生效）:",
            "[✓]".green().bold(),
            assertions.len()
        );
        for assertion in &assertions {
            println!("# @assert {assertion}");
        }
    }
    Ok(())
}
