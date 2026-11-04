use crate::client::AiClient;
use crate::error::{AiError, Result};
use crate::json_util::extract_json_block;
use serde::{Deserialize, Serialize};

/// 每批发送给 LLM 的素材条数（8 × 4KB ≈ 32KB，控制单次上下文）
pub const SYNTHESIS_CHUNK_SIZE: usize = 8;
/// 每块断言建议上限（防单块 token 失控，超出截断）
pub const MAX_ASSERTIONS_PER_ITEM: usize = 8;
/// 连续失败分块熔断阈值
pub const CIRCUIT_BREAKER_THRESHOLD: usize = 2;

const SYNTHESIS_SYSTEM_PROMPT: &str = r#"You are a senior API test engineer authoring regression assertions for `rupost` `.http` files.
Given an ordered JSON array of HTTP request/response exchanges (redacted; `{{var}}` tokens are runtime placeholders — keep them verbatim, never treat them as data), propose regression assertions for EACH exchange.
Output ONLY a valid JSON array (no markdown fences, no prose), one object per input exchange, in the same order, with this exact schema:
[
  {
    "index": 0,
    "assertions": ["status == 200", "body.token exists", "body.role == \"admin\""],
    "captures": ["token from body.token"]
  }
]
Rules:
- assertion format: `<path> <operator> <value>` where path is `status`, `headers.<name>`, `body.<json_path>` or `response.time`; operators: == != > < >= <= contains exists.
- captures format: `<name> from body.<json_path>` or `<name> from header.<name>`; capture values needed by later exchanges (tokens, ids).
- at most 8 assertions per exchange; prefer meaningful business fields over trivial ones.
- use the ACTUAL response structure provided; never invent fields."#;

/// 发送给 LLM 的单条脱敏素材（DTO：纯数据，无 rupost 内部类型）
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SynthesisItem {
    pub index: usize,
    pub method: String,
    pub url: String,
    pub status_code: Option<u16>,
    /// 已脱敏 + 4KB 截断的请求体
    pub request_body_snippet: Option<String>,
    /// 已脱敏 + 4KB 截断的响应体
    pub response_body_snippet: Option<String>,
    /// 批内已有 `@name`（供 AI 理解链路上下文）
    pub available_names: Vec<String>,
}

/// 单条 AI 增强建议（宽容反序列化：漏字段不炸）
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SynthesisAdvice {
    pub index: usize,
    #[serde(default)]
    pub assertions: Vec<String>,
    /// `<name> from body.<path>` / `<name> from header.<name>`
    #[serde(default)]
    pub captures: Vec<String>,
    /// AI 建议的依赖说明（MVP 仅作块注释，不参与排序——时间序天然正确）
    #[serde(default)]
    pub depends_note: Option<String>,
}

/// 合成结果：建议 + 失败分块计数（>0 时调用方打印部分降级警告）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisOutcome {
    pub advice: Vec<SynthesisAdvice>,
    pub failed_chunks: usize,
}

/// LLM 返回的原始建议（index 必填，其余宽容）
#[derive(Debug, Deserialize)]
struct RawAdvice {
    index: usize,
    #[serde(default)]
    assertions: Vec<String>,
    #[serde(default)]
    captures: Vec<String>,
}

fn parse_advice_chunk(raw: &str) -> Result<Vec<SynthesisAdvice>> {
    let clean = extract_json_block(raw);
    let raws: Vec<RawAdvice> = serde_json::from_str(clean).map_err(|e| {
        AiError::TranslationFailed(format!("Invalid JSON in synthesis response ({e}): {clean}"))
    })?;
    if raws.is_empty() {
        return Err(AiError::TranslationFailed(
            "AI returned empty synthesis list".to_string(),
        ));
    }
    Ok(raws
        .into_iter()
        .map(|r| SynthesisAdvice {
            index: r.index,
            assertions: r
                .assertions
                .into_iter()
                .take(MAX_ASSERTIONS_PER_ITEM)
                .collect(),
            captures: r.captures,
            depends_note: None,
        })
        .collect())
}

/// 批量合成器：分块调用 LLM（每块 [`SYNTHESIS_CHUNK_SIZE`] 条），
/// 连续 [`CIRCUIT_BREAKER_THRESHOLD`] 块失败即中止（剩余块计入 failed，交由主包降级）。
pub struct TestSynthesizer<'a> {
    client: &'a dyn AiClient,
}

impl<'a> TestSynthesizer<'a> {
    pub fn new(client: &'a dyn AiClient) -> Self {
        Self { client }
    }

    pub async fn synthesize_batch(&self, items: &[SynthesisItem]) -> Result<SynthesisOutcome> {
        let mut advice = Vec::new();
        let mut failed_chunks = 0usize;
        let mut consecutive_errors = 0usize;

        for chunk in items.chunks(SYNTHESIS_CHUNK_SIZE) {
            if consecutive_errors >= CIRCUIT_BREAKER_THRESHOLD {
                failed_chunks += 1;
                continue;
            }
            let payload = serde_json::to_string_pretty(chunk)?;
            match self
                .client
                .generate_text(SYNTHESIS_SYSTEM_PROMPT, &payload)
                .await
            {
                Ok(raw) => match parse_advice_chunk(&raw) {
                    Ok(mut parsed) => {
                        consecutive_errors = 0;
                        advice.append(&mut parsed);
                    }
                    Err(_) => {
                        consecutive_errors += 1;
                        failed_chunks += 1;
                    }
                },
                Err(_) => {
                    consecutive_errors += 1;
                    failed_chunks += 1;
                }
            }
        }

        Ok(SynthesisOutcome {
            advice,
            failed_chunks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use mockall::mock;

    mock! {
        pub Client {}
        #[async_trait]
        impl AiClient for Client {
            async fn generate_text(&self, system_prompt: &str, user_prompt: &str) -> Result<String>;
        }
    }

    fn item(index: usize) -> SynthesisItem {
        SynthesisItem {
            index,
            method: "GET".to_string(),
            url: format!("https://api.local/v1/users/{index}"),
            status_code: Some(200),
            request_body_snippet: None,
            response_body_snippet: Some(r#"{"token":"[REDACTED]"}"#.to_string()),
            available_names: vec![],
        }
    }

    #[tokio::test]
    async fn test_synthesize_batch_chunks_items() {
        // 3 条 → 1 块（CHUNK 8），恰好 1 次调用
        let mut mock = MockClient::new();
        mock.expect_generate_text()
            .times(1)
            .withf(|_, user| {
                user.contains("\"index\": 0") && user.contains("\"index\": 2")
            })
            .returning(|_, _| {
                Ok(r#"[{"index":0,"assertions":["status == 200"]},{"index":1,"assertions":["status == 200"]},{"index":2,"assertions":["status == 200"]}]"#.to_string())
            });

        let synth = TestSynthesizer::new(&mock);
        let out = synth
            .synthesize_batch(&[item(0), item(1), item(2)])
            .await
            .unwrap();
        assert_eq!(out.advice.len(), 3);
        assert_eq!(out.failed_chunks, 0);
    }

    #[tokio::test]
    async fn test_synthesize_clamps_assertions_and_tolerates_missing_captures() {
        let mut mock = MockClient::new();
        mock.expect_generate_text().times(1).returning(|_, _| {
            Ok(
                r#"[{"index":0,"assertions":["a","b","c","d","e","f","g","h","i","j"]}]"#
                    .to_string(),
            )
        });

        let synth = TestSynthesizer::new(&mock);
        let out = synth.synthesize_batch(&[item(0)]).await.unwrap();
        assert_eq!(out.advice[0].assertions.len(), MAX_ASSERTIONS_PER_ITEM);
        assert!(out.advice[0].captures.is_empty());
    }

    #[tokio::test]
    async fn test_synthesize_circuit_breaker_after_two_failed_chunks() {
        // 17 条 → 3 块；每块都失败：块 1、2 失败后熔断，块 3 直接计入 failed
        let items: Vec<SynthesisItem> = (0..17).map(item).collect();
        let mut mock = MockClient::new();
        mock.expect_generate_text()
            .times(2)
            .returning(|_, _| Ok("not json at all".to_string()));

        let synth = TestSynthesizer::new(&mock);
        let out = synth.synthesize_batch(&items).await.unwrap();
        assert!(out.advice.is_empty());
        assert_eq!(out.failed_chunks, 3);
    }

    #[tokio::test]
    async fn test_synthesize_empty_items_is_ok() {
        let mut mock = MockClient::new();
        mock.expect_generate_text().times(0);
        let synth = TestSynthesizer::new(&mock);
        let out = synth.synthesize_batch(&[]).await.unwrap();
        assert!(out.advice.is_empty());
    }
}
