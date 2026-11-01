use colored::Colorize;
use rupost_ai::{AiClient, OpenAiClient};
use std::sync::Arc;

/// 主库运行时 AI 环境变量配置（零配置原则：密钥仅从环境变量读取，不落盘、不入日志）
#[derive(Debug, Clone, Default)]
pub struct AiRuntimeConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

/// 密钥解析：`RUPOST_AI_API_KEY` 优先于 `OPENAI_API_KEY`，空字符串视同未配置
fn pick_key(rupost_key: Option<String>, openai_key: Option<String>) -> Option<String> {
    let non_empty = |v: Option<String>| v.filter(|k| !k.trim().is_empty());
    non_empty(rupost_key).or_else(|| non_empty(openai_key))
}

impl AiRuntimeConfig {
    /// 从当前进程环境变量加载配置：
    /// - 密钥：`RUPOST_AI_API_KEY` > `OPENAI_API_KEY`（空字符串视同未配置）
    /// - 端点：`RUPOST_AI_BASE_URL`
    /// - 模型：`RUPOST_AI_MODEL`
    pub fn from_env() -> Self {
        Self {
            api_key: pick_key(
                std::env::var("RUPOST_AI_API_KEY").ok(),
                std::env::var("OPENAI_API_KEY").ok(),
            ),
            base_url: std::env::var("RUPOST_AI_BASE_URL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            model: std::env::var("RUPOST_AI_MODEL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }

    /// 若已配置 API Key，则构建 `Arc<dyn AiClient>`；否则返回 `None`
    pub fn into_client(self) -> Option<Arc<dyn AiClient>> {
        let key = self.api_key?;
        Some(Arc::new(OpenAiClient::new(key, self.base_url, self.model)))
    }
}

/// 配置来源（零配置提示与测试断言用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiSource {
    /// 环境变量（显式）
    Env,
    /// 用户级配置文件 `~/.rupost/ai.toml`（显式）
    UserFile,
    /// 本地探测（零配置兜底，如 Ollama 11434）
    LocalProbe,
}

/// 解析完成的 AI 配置（client 为 trait 对象，不派生 Debug/Clone）
pub struct ResolvedAiConfig {
    pub client: Arc<dyn AiClient>,
    pub source: AiSource,
    /// 一次性提示文案：零配置探测命中时非空（如"已使用本地 Ollama (…)"）
    pub notice: Option<String>,
}

/// B4 零 Key 边界：本地回环地址允许占位密钥（Ollama 不校验鉴权），远程端点必须显式 Key
fn is_local_base(base_url: &str) -> bool {
    url::Url::parse(base_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .is_some_and(|h| h == "localhost" || h == "127.0.0.1" || h == "::1" || h == "[::1]")
}

/// 用户级配置文件定位：`$HOME/.rupost/ai.toml`（home 缺失 → None）
fn user_config_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".rupost").join("ai.toml"))
}

#[derive(Debug, Default, serde::Deserialize)]
struct UserAiFile {
    base_url: Option<String>,
    model: Option<String>,
    /// 安全边界（TC-C05）：该字段出现在文件中即拒绝（Key 永不入盘）
    api_key: Option<String>,
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.trim().is_empty())
}

/// 解析用户级 `ai.toml`（TC-C04/C05/C06）：
/// - 合法 → (base_url, model)
/// - 含 api_key → Err 提示（安全边界，整文件拒绝）
/// - 畸形 TOML → Err 提示（调用方降级下一级）
fn parse_user_file(path: &std::path::Path) -> Result<(Option<String>, Option<String>), String> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    let parsed: UserAiFile =
        toml::from_str(&raw).map_err(|e| format!("解析 {} 失败: {e}", path.display()))?;
    if parsed.api_key.is_some() {
        return Err(format!(
            "{} 含 api_key 字段——密钥只允许通过环境变量 `RUPOST_AI_API_KEY` 提供，文件已拒绝加载",
            path.display()
        ));
    }
    Ok((non_empty(parsed.base_url), non_empty(parsed.model)))
}

/// 探测本地 OpenAI 兼容端点（`GET {base}/models`，2s 超时），成功返回 (base_url, model)
async fn probe_local(base: &str) -> Option<(String, String)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()?;
    let url = format!("{}/models", base.trim_end_matches('/'));
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let model = body
        .get("data")
        .and_then(|d| d.get(0))
        .and_then(|m| m.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;
    Some((base.to_string(), model))
}

/// 解析错误：文案即用户可执行指引（TC-C08/US4）
pub const NOT_CONFIGURED_GUIDANCE: &str = "未配置 AI：请任选其一——① `export RUPOST_AI_API_KEY=...`（自建/兼容网关可加 `export RUPOST_AI_BASE_URL=...`）；\
     ② 编写 `~/.rupost/ai.toml`（base_url/model，密钥仍走环境变量）；\
     ③ 安装并启动本地 Ollama（默认探测 http://localhost:11434/v1）";

/// B4 配置链（TC-C01–C11）：环境变量 > `~/.rupost/ai.toml` > 本地探测。
/// `probe_override`：测试钩子（`RUPOST_AI_PROBE_URL`），替换默认探测候选以保测试确定性。
pub async fn resolve_with(
    env: &AiRuntimeConfig,
    file_path: Option<&std::path::Path>,
    probe_override: Option<&str>,
) -> std::result::Result<ResolvedAiConfig, String> {
    // ── 一级：环境变量（有密钥即最优先；端点缺省走 OpenAiClient 默认值，
    //    与 run_ai_assert 时代的 key-only 语义对齐——合并回归修复） ──
    if let Some(key) = &env.api_key {
        return Ok(ResolvedAiConfig {
            client: Arc::new(OpenAiClient::new(
                key.clone(),
                env.base_url.clone(),
                env.model.clone(),
            )),
            source: AiSource::Env,
            notice: None,
        });
    }

    // ── 二级：用户级配置文件 ──
    if let Some(path) = file_path
        && path.exists()
    {
        match parse_user_file(path) {
            Ok((base_url, model)) => {
                let base =
                    base_url.ok_or_else(|| format!("{} 缺少 base_url 字段", path.display()))?;
                let key = match &env.api_key {
                    Some(k) => k.clone(),
                    None if is_local_base(&base) => "local".to_string(), // TC-C07 占位
                    None => return Err(NOT_CONFIGURED_GUIDANCE.to_string()), // TC-C08
                };
                return Ok(ResolvedAiConfig {
                    client: Arc::new(OpenAiClient::new(key, Some(base), model)),
                    source: AiSource::UserFile,
                    notice: None,
                });
            }
            Err(msg) => {
                // TC-C05/C06：文件问题 → 警告并降级下一级（不中断）
                eprintln!("{} {}", "⚠".yellow(), msg);
            }
        }
    }

    // ── 三级：本地探测（TC-C09/C10/C11） ──
    let candidate = probe_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| "http://localhost:11434/v1".to_string());
    if let Some((base, model)) = probe_local(&candidate).await {
        let notice = format!("未检测到显式 AI 配置，已使用本地探测端点 {base}（模型: {model}）");
        return Ok(ResolvedAiConfig {
            client: Arc::new(OpenAiClient::new(
                "local".to_string(),
                Some(base),
                Some(model),
            )),
            source: AiSource::LocalProbe,
            notice: Some(notice),
        });
    }

    Err(NOT_CONFIGURED_GUIDANCE.to_string())
}

/// 生产入口：env + 默认文件路径 + `RUPOST_AI_PROBE_URL` 覆盖（该变量为测试钩子，不入用户文档）
pub async fn resolve() -> std::result::Result<ResolvedAiConfig, String> {
    let file = user_config_path();
    let probe = std::env::var("RUPOST_AI_PROBE_URL").ok();
    resolve_with(
        &AiRuntimeConfig::from_env(),
        file.as_deref(),
        probe.as_deref(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pick_key_prefers_rupost_and_ignores_empty() {
        assert_eq!(
            pick_key(Some("a".to_string()), Some("b".to_string())),
            Some("a".to_string())
        );
        // 空字符串与纯空白视同未配置
        assert_eq!(
            pick_key(Some("  ".to_string()), Some("b".to_string())),
            Some("b".to_string())
        );
        assert_eq!(pick_key(Some(String::new()), None), None);
        assert_eq!(pick_key(None, Some("b".to_string())), Some("b".to_string()));
        assert_eq!(pick_key(None, None), None);
    }

    #[test]
    fn test_into_client_requires_key() {
        let unconfigured = AiRuntimeConfig::default();
        assert!(!unconfigured.is_configured());
        assert!(unconfigured.into_client().is_none());

        let configured = AiRuntimeConfig {
            api_key: Some("k".to_string()),
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
            model: Some("m".to_string()),
        };
        assert!(configured.is_configured());
        assert!(configured.into_client().is_some());
    }

    // ── B4 链测试（TC-C01–C11） ──

    fn write_file(path: &std::path::Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn env_cfg(key: Option<&str>, base: Option<&str>, model: Option<&str>) -> AiRuntimeConfig {
        AiRuntimeConfig {
            api_key: key.map(|s| s.to_string()),
            base_url: base.map(|s| s.to_string()),
            model: model.map(|s| s.to_string()),
        }
    }

    #[tokio::test]
    async fn test_env_beats_file_and_probe() {
        // TC-C01：env 优先于 file/probe
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("ai.toml");
        write_file(
            &cfg_path,
            "base_url = \"http://file-url/v1\"\nmodel = \"file-model\"\n",
        );

        let resolved = resolve_with(
            &env_cfg(
                Some("env-key"),
                Some("http://env-url/v1"),
                Some("env-model"),
            ),
            Some(&cfg_path),
            Some("http://127.0.0.1:1"), // 探测必须不可达——env 胜出时不应触达
        )
        .await
        .unwrap();
        assert_eq!(resolved.source, AiSource::Env);
        assert!(resolved.notice.is_none());
    }

    #[tokio::test]
    async fn test_file_beats_probe() {
        // TC-C02：file 优先于 probe
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("ai.toml");
        write_file(
            &cfg_path,
            "base_url = \"http://127.0.0.1:1/v1\"\nmodel = \"file-model\"\n",
        );

        let resolved = resolve_with(
            &env_cfg(None, None, None),
            Some(&cfg_path),
            Some("http://127.0.0.1:1"),
        )
        .await
        .unwrap();
        assert_eq!(resolved.source, AiSource::UserFile);
    }

    #[tokio::test]
    async fn test_file_rejects_api_key_field() {
        // TC-C05：安全边界——ai.toml 含 api_key 即拒绝该文件
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("ai.toml");
        write_file(
            &cfg_path,
            "base_url = \"http://x/v1\"\napi_key = \"leaked\"\n",
        );
        let res = resolve_with(&env_cfg(None, None, None), Some(&cfg_path), None).await;
        assert!(
            res.is_err() || res.unwrap().source == AiSource::LocalProbe,
            "含 api_key 的文件不得被静默采纳"
        );
    }

    #[tokio::test]
    async fn test_malformed_file_degrades_with_warning() {
        // TC-C06：畸形 TOML → 降级到探测（探测不可达 → 报错）
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("ai.toml");
        write_file(&cfg_path, "not [valid toml ===");
        let res = resolve_with(
            &env_cfg(None, None, None),
            Some(&cfg_path),
            Some("http://127.0.0.1:1"),
        )
        .await;
        assert!(res.is_err(), "降级后探测失败应报错");
    }

    #[tokio::test]
    async fn test_local_base_without_key_gets_placeholder() {
        // TC-C07：本地端点允许占位 Key（Ollama 不校验鉴权）
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("ai.toml");
        write_file(&cfg_path, "base_url = \"http://localhost:9999/v1\"\n");
        let resolved = resolve_with(&env_cfg(None, None, None), Some(&cfg_path), None)
            .await
            .unwrap();
        assert_eq!(resolved.source, AiSource::UserFile);
    }

    #[tokio::test]
    async fn test_remote_base_without_key_rejected() {
        // TC-C08：远程端点无 Key → 拒绝并给指引
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("ai.toml");
        write_file(&cfg_path, "base_url = \"https://api.example.com/v1\"\n");
        let res = resolve_with(&env_cfg(None, None, None), Some(&cfg_path), None).await;
        let msg = res.err().expect("remote-without-key should fail");
        assert!(msg.contains("RUPOST_AI_API_KEY"), "指引应含变量名: {msg}");
    }

    #[tokio::test]
    async fn test_probe_success_picks_first_model() {
        // TC-C09：探测成功（注入 wiremock）→ 取 data[0].id
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": [{"id": "probe-model"}]})),
            )
            .mount(&server)
            .await;

        let resolved = resolve_with(
            &env_cfg(None, None, None),
            None,
            Some(&format!("{}/v1", server.uri())),
        )
        .await
        .unwrap();
        assert_eq!(resolved.source, AiSource::LocalProbe);
        assert!(
            resolved
                .notice
                .as_deref()
                .unwrap()
                .contains(server.uri().as_str())
        );
    }

    #[tokio::test]
    async fn test_probe_failure_returns_guidance() {
        // TC-C10：探测不可达 → 指引
        let res = resolve_with(&env_cfg(None, None, None), None, Some("http://127.0.0.1:1")).await;
        assert!(
            res.err()
                .expect("probe failure should yield guidance")
                .contains("Ollama")
        );
    }

    #[tokio::test]
    async fn test_probe_non_json_or_error_is_unavailable() {
        // TC-C11：探测端点 500 → 不可用
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let res = resolve_with(
            &env_cfg(None, None, None),
            None,
            Some(&format!("{}/v1", server.uri())),
        )
        .await;
        assert!(res.is_err());
    }

    #[test]
    fn test_local_base_detection() {
        assert!(is_local_base("http://localhost:11434/v1"));
        assert!(is_local_base("http://127.0.0.1:8080"));
        assert!(!is_local_base("https://api.example.com/v1"));
    }
}
