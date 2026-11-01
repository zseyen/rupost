use crate::client::AiClient;
use crate::error::{AiError, Result};
use crate::json_util::extract_json_block;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashSet;

const TRIAGE_SYSTEM_PROMPT: &str = r#"You are a CI Failure Triage Engine for `rupost`.
Given a JSON array of deterministic failure signature buckets (`bucket_id`, `signature`, `default_category`, `count`, `sample_requests`, `sample_error`), merge buckets that share the same underlying root cause into clusters.
Output ONLY a valid JSON array (no markdown fences, no extra prose) where each element has this schema:
[
  {
    "category": "Auth | Upstream5xx | SchemaDrift | AssertionMismatch | NetworkAndTls | VariableOrConfig",
    "title": "Short cluster title",
    "bucket_ids": [0, 2],
    "root_cause": "Concise explanation of the shared root cause",
    "fix_suggestion": "Actionable remediation step"
  }
]
Every input `bucket_id` MUST appear in exactly one cluster."#;

/// 失败根因分类枚举：兼容 PRD 别名（大小写/下划线变体），未知分类无损落入 `Other(String)`，绝不报 ParseError
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureCategory {
    Auth,
    Upstream5xx,
    SchemaDrift,
    AssertionMismatch,
    NetworkAndTls,
    VariableOrConfig,
    Other(String),
}

impl FailureCategory {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Auth => "Auth",
            Self::Upstream5xx => "Upstream5xx",
            Self::SchemaDrift => "SchemaDrift",
            Self::AssertionMismatch => "AssertionMismatch",
            Self::NetworkAndTls => "NetworkAndTls",
            Self::VariableOrConfig => "VariableOrConfig",
            Self::Other(s) => s.as_str(),
        }
    }

    pub fn from_loose_str(s: &str) -> Self {
        let norm = s.trim();
        match norm.to_ascii_lowercase().as_str() {
            "auth"
            | "authentication"
            | "authorization"
            | "authenticationexpired"
            | "401"
            | "403" => Self::Auth,
            "upstream5xx" | "upstream_5xx" | "internalservererror" | "server_error" | "5xx" => {
                Self::Upstream5xx
            }
            "schemadrift" | "schema_drift" | "contractmismatch" | "schema" => Self::SchemaDrift,
            "assertionmismatch" | "assertion_mismatch" | "assertionlogicerror" | "assertion" => {
                Self::AssertionMismatch
            }
            "networkandtls"
            | "network_and_tls"
            | "infrastructuretimeout"
            | "network"
            | "tls"
            | "dns"
            | "timeout" => Self::NetworkAndTls,
            "variableorconfig" | "variable_or_config" | "variable" | "config" => {
                Self::VariableOrConfig
            }
            _ => Self::Other(if norm.is_empty() {
                "Uncategorized".to_string()
            } else {
                norm.to_string()
            }),
        }
    }
}

impl Serialize for FailureCategory {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FailureCategory {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_loose_str(&s))
    }
}

/// 确定性签名分桶（由主库规则层在调用 AI 前生成，单次最多传 20 个桶给 LLM）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignatureBucket {
    pub bucket_id: usize,
    pub signature: String,
    pub default_category: FailureCategory,
    pub count: usize,
    /// 代表性失败请求标识（如 `["auth.http#1 (Login)"]`，最多 5 条）
    pub sample_requests: Vec<String>,
    pub sample_error: String,
    pub sample_snippet: Option<String>,
    /// 该桶全部失败请求标识（不发给 LLM，归并后还原完整 affected_requests）
    #[serde(default, skip_serializing)]
    pub all_requests: Vec<String>,
    /// 规则引擎默认原因（无 Key 降级或漏桶兜底用）
    #[serde(default, skip_serializing)]
    pub default_root_cause: String,
    /// 规则引擎默认修复建议
    #[serde(default, skip_serializing)]
    pub default_fix_suggestion: String,
}

/// 最终输出的单个失败根因簇
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailureCluster {
    pub category: FailureCategory,
    pub title: String,
    pub count: usize,
    pub affected_requests: Vec<String>,
    pub root_cause: String,
    pub fix_suggestion: String,
}

/// 完整分诊报告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriageReport {
    pub total_requests: usize,
    pub failed_requests: usize,
    pub cluster_count: usize,
    pub ai_enhanced: bool,
    pub clusters: Vec<FailureCluster>,
}

/// LLM 返回的原始簇 DTO（注记式：只能引用 bucket_id 并贴标签，不能移动成员）
#[derive(Debug, Deserialize)]
struct RawAiCluster {
    category: FailureCategory,
    title: String,
    bucket_ids: Vec<usize>,
    root_cause: String,
    #[serde(default)]
    fix_suggestion: String,
}

/// 将单个 `SignatureBucket` 直接转换为确定性 `FailureCluster`（B1 离线基线与漏桶兜底）
pub fn bucket_to_deterministic_cluster(bucket: &SignatureBucket) -> FailureCluster {
    let requests = if bucket.all_requests.is_empty() {
        bucket.sample_requests.clone()
    } else {
        bucket.all_requests.clone()
    };
    FailureCluster {
        category: bucket.default_category.clone(),
        title: bucket.signature.clone(),
        count: bucket.count,
        affected_requests: requests,
        root_cause: bucket.default_root_cause.clone(),
        fix_suggestion: bucket.default_fix_suggestion.clone(),
    }
}

pub struct FailureClassifier<'a> {
    client: &'a dyn AiClient,
}

impl<'a> FailureClassifier<'a> {
    pub fn new(client: &'a dyn AiClient) -> Self {
        Self { client }
    }

    /// 对签名桶做 AI 语义归并，严格执行**分桶 ID 守恒**：
    /// - 每个输入桶最多被消费一次（重复引用忽略，不重复计数）
    /// - LLM 遗漏的桶自动补为确定性簇——失败总数 100% 守恒
    pub async fn classify(&self, buckets: &[SignatureBucket]) -> Result<Vec<FailureCluster>> {
        if buckets.is_empty() {
            return Ok(Vec::new());
        }

        let payload = serde_json::to_string_pretty(buckets)?;
        let raw_reply = self
            .client
            .generate_text(TRIAGE_SYSTEM_PROMPT, &payload)
            .await?;
        let clean_json = extract_json_block(&raw_reply);

        let raw_clusters: Vec<RawAiCluster> = serde_json::from_str(clean_json).map_err(|e| {
            AiError::TranslationFailed(format!(
                "Invalid JSON in triage response ({e}): {clean_json}"
            ))
        })?;

        if raw_clusters.is_empty() {
            return Err(AiError::TranslationFailed(
                "AI returned empty cluster list".to_string(),
            ));
        }

        let mut consumed_ids = HashSet::new();
        let mut final_clusters = Vec::new();

        for rc in raw_clusters {
            let mut count = 0;
            let mut affected = Vec::new();
            for bid in rc.bucket_ids {
                if consumed_ids.contains(&bid) {
                    continue;
                }
                if let Some(b) = buckets.iter().find(|b| b.bucket_id == bid) {
                    consumed_ids.insert(bid);
                    count += b.count;
                    if b.all_requests.is_empty() {
                        affected.extend(b.sample_requests.iter().cloned());
                    } else {
                        affected.extend(b.all_requests.iter().cloned());
                    }
                }
            }
            if count > 0 {
                final_clusters.push(FailureCluster {
                    category: rc.category,
                    title: rc.title,
                    count,
                    affected_requests: affected,
                    root_cause: rc.root_cause,
                    fix_suggestion: rc.fix_suggestion,
                });
            }
        }

        // 守恒兜底：LLM 遗漏的桶自动补为确定性簇
        for b in buckets {
            if !consumed_ids.contains(&b.bucket_id) {
                final_clusters.push(bucket_to_deterministic_cluster(b));
            }
        }

        Ok(final_clusters)
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

    fn sample_buckets() -> Vec<SignatureBucket> {
        vec![
            SignatureBucket {
                bucket_id: 0,
                signature: "HTTP 401 @ /v1/*".to_string(),
                default_category: FailureCategory::Auth,
                count: 3,
                sample_requests: vec!["a.http#1".to_string()],
                sample_error: "status == 200 (actual 401)".to_string(),
                sample_snippet: None,
                all_requests: vec![
                    "a.http#1".to_string(),
                    "a.http#2".to_string(),
                    "a.http#3".to_string(),
                ],
                default_root_cause: "HTTP 401 Unauthorized".to_string(),
                default_fix_suggestion: "Check auth token".to_string(),
            },
            SignatureBucket {
                bucket_id: 1,
                signature: "HTTP 502 @ /v1/pay".to_string(),
                default_category: FailureCategory::Upstream5xx,
                count: 2,
                sample_requests: vec!["b.http#1".to_string()],
                sample_error: "502 Bad Gateway".to_string(),
                sample_snippet: None,
                all_requests: vec!["b.http#1".to_string(), "b.http#2".to_string()],
                default_root_cause: "Upstream 502".to_string(),
                default_fix_suggestion: "Check upstream gateway".to_string(),
            },
        ]
    }

    #[tokio::test]
    async fn test_classify_conserves_missing_buckets_and_unknown_category() {
        let mut mock = MockClient::new();
        // LLM 只返回 bucket_id 0 且用了未知分类名，漏掉 bucket_id 1
        mock.expect_generate_text().times(1).returning(|_, _| {
            Ok(r#"[{"category":"CustomAuthError","title":"Expired Token","bucket_ids":[0],"root_cause":"Token expired","fix_suggestion":"Re-login"}]"#.to_string())
        });

        let classifier = FailureClassifier::new(&mock);
        let clusters = classifier.classify(&sample_buckets()).await.unwrap();

        // 2 个簇：bucket 0（AI 归并）+ bucket 1（守恒自动补齐）
        assert_eq!(clusters.len(), 2);
        let total: usize = clusters.iter().map(|c| c.count).sum();
        assert_eq!(total, 5);
        assert_eq!(
            clusters[0].category,
            FailureCategory::Other("CustomAuthError".to_string())
        );
        assert_eq!(clusters[1].category, FailureCategory::Upstream5xx);
    }

    #[tokio::test]
    async fn test_classify_ignores_duplicate_and_hallucinated_bucket_ids() {
        let mut mock = MockClient::new();
        // bucket 0 被重复引用（不重复计数），bucket 99 是幻觉 ID（忽略），bucket 1 漏掉（守恒补齐）
        mock.expect_generate_text().times(1).returning(|_, _| {
            Ok(r#"[{"category":"Auth","title":"A","bucket_ids":[0,0,99],"root_cause":"r","fix_suggestion":"f"},{"category":"Auth","title":"B","bucket_ids":[0],"root_cause":"r2","fix_suggestion":"f2"}]"#.to_string())
        });

        let classifier = FailureClassifier::new(&mock);
        let clusters = classifier.classify(&sample_buckets()).await.unwrap();

        let total: usize = clusters.iter().map(|c| c.count).sum();
        assert_eq!(total, 5, "计数必须 100% 守恒：重复与幻觉 ID 不得影响总数");
        assert_eq!(clusters[0].count, 3);
        assert_eq!(clusters[1].count, 2, "漏掉的 bucket 1 应由守恒兜底补齐");
    }

    #[tokio::test]
    async fn test_classify_rejects_non_json_output() {
        let mut mock = MockClient::new();
        mock.expect_generate_text()
            .times(1)
            .returning(|_, _| Ok("prose only".to_string()));

        let classifier = FailureClassifier::new(&mock);
        assert!(matches!(
            classifier.classify(&sample_buckets()).await,
            Err(AiError::TranslationFailed(_))
        ));
    }

    #[tokio::test]
    async fn test_classify_rejects_empty_cluster_list() {
        // LLM 返回 "[]" 视为失败 → 上层降级确定性基线
        let mut mock = MockClient::new();
        mock.expect_generate_text()
            .times(1)
            .returning(|_, _| Ok("[]".to_string()));
        let classifier = FailureClassifier::new(&mock);
        assert!(matches!(
            classifier.classify(&sample_buckets()).await,
            Err(AiError::TranslationFailed(_))
        ));
    }

    #[tokio::test]
    async fn test_classify_empty_buckets_returns_empty() {
        let mut mock = MockClient::new();
        mock.expect_generate_text().times(0);
        let classifier = FailureClassifier::new(&mock);
        assert!(classifier.classify(&[]).await.unwrap().is_empty());
    }

    #[test]
    fn test_failure_category_loose_deserialization() {
        assert_eq!(
            serde_json::from_str::<FailureCategory>("\"InfrastructureTimeout\"").unwrap(),
            FailureCategory::NetworkAndTls
        );
        assert_eq!(
            serde_json::from_str::<FailureCategory>("\"ContractMismatch\"").unwrap(),
            FailureCategory::SchemaDrift
        );
        assert_eq!(
            serde_json::from_str::<FailureCategory>("\"auth\"").unwrap(),
            FailureCategory::Auth
        );
        assert_eq!(
            serde_json::from_str::<FailureCategory>("\"Weird Stuff\"").unwrap(),
            FailureCategory::Other("Weird Stuff".to_string())
        );
        // 序列化回字符串
        assert_eq!(
            serde_json::to_string(&FailureCategory::Upstream5xx).unwrap(),
            "\"Upstream5xx\""
        );
    }

    #[test]
    fn test_signature_bucket_serialization_skips_internal_fields() {
        let b = &sample_buckets()[0];
        let json = serde_json::to_string(b).unwrap();
        assert!(!json.contains("all_requests"));
        assert!(!json.contains("default_root_cause"));
        assert!(json.contains("\"bucket_id\":0"));
    }
}
