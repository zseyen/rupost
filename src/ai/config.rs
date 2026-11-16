use crate::diagnostics::{StderrWarningSink, WarningSeverity, WarningSink};
use crate::platform;
use rupost_ai::{AiClient, OpenAiClient};
use std::collections::HashMap;
use std::fmt;
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
    /// 本次解析到的实际密钥（仅用于注册进动态脱敏词表；`"local"` 占位符为 None）
    pub resolved_secret: Option<String>,
}

/// B4 零 Key 边界：本地回环地址允许占位密钥（Ollama 不校验鉴权），远程端点必须显式 Key
fn is_local_base(base_url: &str) -> bool {
    crate::http::url::is_loopback_host(base_url)
}

/// 用户级配置文件定位：`$HOME/.rupost/ai.toml`（home 缺失 → None）
fn user_config_path() -> Option<std::path::PathBuf> {
    platform::rupost_path("ai.toml")
}

#[derive(Debug, Default, Clone, serde::Deserialize)]
struct AiProfileConfig {
    base_url: Option<String>,
    model: Option<String>,
    api_key_env: Option<String>,
    /// 安全边界（TC-C05）：profile 内出现明文 api_key 同样拒绝
    api_key: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct UserAiFile {
    base_url: Option<String>,
    model: Option<String>,
    api_key_env: Option<String>,
    profile: Option<String>,
    #[serde(default)]
    profiles: HashMap<String, AiProfileConfig>,
    /// 安全边界（TC-C05）：该字段出现在文件中即拒绝（Key 永不入盘）
    api_key: Option<String>,
}

#[derive(Default, serde::Deserialize)]
struct AiCredentialEntry {
    api_key: Option<String>,
}

/// 用户本机私有凭据文件 `$HOME/.rupost/credentials.toml`（禁用 Serialize，脱敏 Debug）。
#[derive(Default, serde::Deserialize)]
pub struct AiCredentialsFile {
    api_key: Option<String>,
    #[serde(default)]
    profiles: HashMap<String, AiCredentialEntry>,
}

impl fmt::Debug for AiCredentialsFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiCredentialsFile")
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("profiles_count", &self.profiles.len())
            .finish()
    }
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.trim().is_empty())
}

/// 包装 `AiClient`，自动掩码输出文本与错误消息中的解析密钥，防止向上层日志/终端泄露。
struct RedactingAiClient {
    inner: Arc<dyn AiClient>,
    secret: String,
}

#[async_trait::async_trait]
impl AiClient for RedactingAiClient {
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> rupost_ai::Result<String> {
        let mask = |s: &str| {
            crate::security::redact::redact_explicit_secrets(s, std::slice::from_ref(&self.secret))
        };
        match self.inner.generate_text(system_prompt, user_prompt).await {
            Ok(text) => Ok(mask(&text)),
            Err(rupost_ai::AiError::ApiError { status, message }) => {
                Err(rupost_ai::AiError::ApiError {
                    status,
                    message: mask(&message),
                })
            }
            Err(rupost_ai::AiError::TranslationFailed(msg)) => {
                Err(rupost_ai::AiError::TranslationFailed(mask(&msg)))
            }
            Err(other) => Err(rupost_ai::AiError::TranslationFailed(mask(
                &other.to_string(),
            ))),
        }
    }
}

fn wrap_client_with_secret(client: Arc<dyn AiClient>, secret: Option<&str>) -> Arc<dyn AiClient> {
    // 仅对长度 >= 8 的真实密钥启用子串掩码，避免测试中使用的短占位 Key（如 "k"）误伤正常单词
    match secret.filter(|s| s.trim().len() >= 8) {
        Some(s) => Arc::new(RedactingAiClient {
            inner: client,
            secret: s.to_string(),
        }),
        None => client,
    }
}

#[derive(Debug, Default)]
struct ParsedUserAiConfig {
    base_url: Option<String>,
    model: Option<String>,
    api_key_env: Option<String>,
    active_profile: Option<String>,
}

fn build_parsed_ai_config(
    parsed: UserAiFile,
    source_disp: &str,
) -> Result<ParsedUserAiConfig, String> {
    if parsed.api_key.is_some() || parsed.profiles.values().any(|p| p.api_key.is_some()) {
        return Err(format!(
            "{source_disp} 含 api_key 字段——密钥只允许通过环境变量（`RUPOST_AI_API_KEY` / `api_key_env`）或 `~/.rupost/credentials.toml` 提供，文件已拒绝加载"
        ));
    }
    let active = non_empty(parsed.profile);
    let profile_cfg = active.as_ref().and_then(|name| parsed.profiles.get(name));
    Ok(ParsedUserAiConfig {
        base_url: profile_cfg
            .and_then(|p| non_empty(p.base_url.clone()))
            .or_else(|| non_empty(parsed.base_url)),
        model: profile_cfg
            .and_then(|p| non_empty(p.model.clone()))
            .or_else(|| non_empty(parsed.model)),
        api_key_env: profile_cfg
            .and_then(|p| non_empty(p.api_key_env.clone()))
            .or_else(|| non_empty(parsed.api_key_env)),
        active_profile: active,
    })
}

/// 解析用户级 `ai.toml`（TC-C04/C05/C06 + 多 profile 支持）
fn parse_user_file(path: &std::path::Path) -> Result<ParsedUserAiConfig, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    let parsed: UserAiFile =
        toml::from_str(&raw).map_err(|e| format!("解析 {} 失败: {e}", path.display()))?;
    build_parsed_ai_config(parsed, &path.display().to_string())
}

/// 解析项目级 `./rupost.toml` 的 `[ai]` 段（同样受 TC-C05 禁明文 `api_key` 门禁约束）
fn parse_project_rupost_ai_section(
    path: &std::path::Path,
) -> Option<Result<ParsedUserAiConfig, String>> {
    #[derive(serde::Deserialize)]
    struct RupostAiWrapper {
        ai: Option<UserAiFile>,
    }
    let raw = std::fs::read_to_string(path).ok()?;
    let wrapper: RupostAiWrapper = match toml::from_str(&raw) {
        Ok(w) => w,
        Err(_) => return None,
    };
    let ai = wrapper.ai?;
    Some(build_parsed_ai_config(
        ai,
        &format!("{} [ai]", path.display()),
    ))
}

/// 安全加载本机私有凭据 `$HOME/.rupost/credentials.toml`（5 层安全门禁）：
/// 1. 仅从显式传入路径（由 `platform::rupost_path("credentials.toml")` 定位，绝不扫工作区）；
/// 2. 拒绝符号链接与非普通文件；
/// 3. Unix 下强制 `uid == geteuid()` 且 `mode & 0o077 == 0`（如 `0o644` 拒绝并提示 `chmod 600`）；
/// 4. Unix 使用 `O_NOFOLLOW` 打开；
/// 5. 内存防泄露（脱敏 Debug、禁用 Serialize）。
pub fn load_credentials_file_with_sink(
    path: &std::path::Path,
    sink: &dyn WarningSink,
) -> Option<AiCredentialsFile> {
    if std::fs::symlink_metadata(path).is_err() {
        return None;
    }
    if !crate::security::fs::is_owner_only_regular_file(path) {
        sink.emit(
            WarningSeverity::Security,
            "ai::config",
            &format!(
                "拒绝读取权限过宽或不可信的凭据文件 {}（请执行 `chmod 600 {}`）",
                path.display(),
                path.display()
            ),
        );
        return None;
    }
    #[cfg(unix)]
    let raw = {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(f) => f,
            Err(e) => {
                sink.emit(
                    WarningSeverity::Warn,
                    "ai::config",
                    &format!("打开 {} 失败: {e}", path.display()),
                );
                return None;
            }
        };
        let mut buf = String::new();
        if let Err(e) = file.read_to_string(&mut buf) {
            sink.emit(
                WarningSeverity::Warn,
                "ai::config",
                &format!("读取 {} 失败: {e}", path.display()),
            );
            return None;
        }
        buf
    };
    #[cfg(not(unix))]
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            sink.emit(
                WarningSeverity::Warn,
                "ai::config",
                &format!("读取 {} 失败: {e}", path.display()),
            );
            return None;
        }
    };
    match toml::from_str::<AiCredentialsFile>(&raw) {
        Ok(creds) => Some(creds),
        Err(e) => {
            sink.emit(
                WarningSeverity::Warn,
                "ai::config",
                &format!("解析 {} 失败: {e}", path.display()),
            );
            None
        }
    }
}

impl AiCredentialsFile {
    fn resolve_key(&self, profile: Option<&str>) -> Option<String> {
        if let Some(p) = profile
            && let Some(entry) = self.profiles.get(p)
            && let Some(k) = non_empty(entry.api_key.clone())
        {
            return Some(k);
        }
        non_empty(self.api_key.clone())
    }
}

/// 探测本地 OpenAI 兼容端点（`GET {base}/models`，2s 超时），成功返回 (base_url, model)
async fn probe_local(base: &str) -> Option<(String, String)> {
    let client =
        crate::http::client::build_plain_http_client(std::time::Duration::from_secs(2)).ok()?;
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
/// 纯函数隔离：仅使用传入 `env`，绝不读取宿主机全局环境变量。
pub async fn resolve_with(
    env: &AiRuntimeConfig,
    file_path: Option<&std::path::Path>,
    probe_override: Option<&str>,
) -> std::result::Result<ResolvedAiConfig, String> {
    resolve_with_env_lookup(
        env,
        file_path,
        None,
        probe_override,
        &|_| None,
        &StderrWarningSink,
    )
    .await
}

/// 纯函数可注入版配置解析（完全隔离宿主机环境变量，供单元测试与确定性调用使用）。
pub async fn resolve_with_options(
    env: &AiRuntimeConfig,
    file_path: Option<&std::path::Path>,
    credentials_path: Option<&std::path::Path>,
    probe_override: Option<&str>,
    sink: &dyn WarningSink,
) -> std::result::Result<ResolvedAiConfig, String> {
    resolve_with_env_lookup(
        env,
        file_path,
        credentials_path,
        probe_override,
        &|_| None,
        sink,
    )
    .await
}

/// 完整解析优先级（支持显式注入 `env_lookup`）：
/// `RUPOST_AI_API_KEY` > `[ai].api_key_env` > `OPENAI_API_KEY` > `~/.rupost/credentials.toml` > 本地 `"local"` 占位。
pub async fn resolve_with_env_lookup(
    env: &AiRuntimeConfig,
    file_path: Option<&std::path::Path>,
    credentials_path: Option<&std::path::Path>,
    probe_override: Option<&str>,
    env_lookup: &(dyn Fn(&str) -> Option<String> + Sync),
    sink: &dyn WarningSink,
) -> std::result::Result<ResolvedAiConfig, String> {
    resolve_with_parsed_and_env(
        env,
        file_path.and_then(|path| {
            if path.exists() {
                match parse_user_file(path) {
                    Ok(cfg) => Some((path.display().to_string(), cfg)),
                    Err(msg) => {
                        sink.emit(WarningSeverity::Warn, "ai::config", &msg);
                        None
                    }
                }
            } else {
                None
            }
        }),
        credentials_path,
        probe_override,
        env_lookup,
        sink,
    )
    .await
}

async fn resolve_with_parsed_and_env(
    env: &AiRuntimeConfig,
    parsed_entry: Option<(String, ParsedUserAiConfig)>,
    credentials_path: Option<&std::path::Path>,
    probe_override: Option<&str>,
    env_lookup: &(dyn Fn(&str) -> Option<String> + Sync),
    sink: &dyn WarningSink,
) -> std::result::Result<ResolvedAiConfig, String> {
    let rupost_env_key = non_empty(env_lookup("RUPOST_AI_API_KEY"));
    let openai_env_key = non_empty(env_lookup("OPENAI_API_KEY"));

    if let Some((path_disp, cfg)) = parsed_entry {
        let custom_env_key = cfg
            .api_key_env
            .as_deref()
            .and_then(|name| non_empty(env_lookup(name)));

        let creds_key = || {
            credentials_path
                .and_then(|p| load_credentials_file_with_sink(p, sink))
                .and_then(|creds| creds.resolve_key(cfg.active_profile.as_deref()))
        };

        let merged_base = env.base_url.clone().or(cfg.base_url);
        let merged_model = env.model.clone().or(cfg.model);

        // 若未配置 base_url 但 env.api_key 存在，允许走默认 OpenAiClient 端点（与 env-only 语义兼容）
        let (key, resolved_secret, source) = if let Some(k) = rupost_env_key {
            (k.clone(), Some(k), AiSource::Env)
        } else if let Some(k) = custom_env_key {
            (k.clone(), Some(k), AiSource::UserFile)
        } else if let Some(k) = openai_env_key {
            (k.clone(), Some(k), AiSource::Env)
        } else if let Some(k) = &env.api_key {
            (k.clone(), Some(k.clone()), AiSource::Env)
        } else if let Some(k) = creds_key() {
            (k.clone(), Some(k), AiSource::UserFile)
        } else if let Some(base) = &merged_base
            && is_local_base(base)
        {
            ("local".to_string(), None, AiSource::UserFile)
        } else if merged_base.is_none() {
            return Err(format!("{path_disp} 缺少 base_url 字段"));
        } else {
            return Err(NOT_CONFIGURED_GUIDANCE.to_string());
        };

        let raw_client: Arc<dyn AiClient> =
            Arc::new(OpenAiClient::new(key, merged_base, merged_model));
        return Ok(ResolvedAiConfig {
            client: wrap_client_with_secret(raw_client, resolved_secret.as_deref()),
            source,
            notice: None,
            resolved_secret,
        });
    }

    // ── 无有效配置文件：纯 env 一级 ──
    if let Some(key) = rupost_env_key
        .or(openai_env_key)
        .or_else(|| env.api_key.clone())
    {
        let raw_client: Arc<dyn AiClient> = Arc::new(OpenAiClient::new(
            key.clone(),
            env.base_url.clone(),
            env.model.clone(),
        ));
        return Ok(ResolvedAiConfig {
            client: wrap_client_with_secret(raw_client, Some(&key)),
            source: AiSource::Env,
            notice: None,
            resolved_secret: Some(key),
        });
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
            resolved_secret: None,
        });
    }

    Err(NOT_CONFIGURED_GUIDANCE.to_string())
}

/// 生产入口：env + `~/.rupost/ai.toml`（或 `./rupost.toml [ai]`）+ `~/.rupost/credentials.toml` + `RUPOST_AI_PROBE_URL` 覆盖
pub async fn resolve() -> std::result::Result<ResolvedAiConfig, String> {
    let sink = StderrWarningSink;
    let user_file = user_config_path();
    let parsed_entry = if let Some(path) = user_file.as_deref().filter(|p| p.exists()) {
        match parse_user_file(path) {
            Ok(cfg) => Some((path.display().to_string(), cfg)),
            Err(msg) => {
                sink.emit(WarningSeverity::Warn, "ai::config", &msg);
                None
            }
        }
    } else if let Some(proj_toml) = crate::variable::ConfigLoader::find_config_path() {
        match parse_project_rupost_ai_section(&proj_toml) {
            Some(Ok(cfg)) => Some((format!("{} [ai]", proj_toml.display()), cfg)),
            Some(Err(msg)) => {
                sink.emit(WarningSeverity::Warn, "ai::config", &msg);
                None
            }
            None => None,
        }
    } else {
        None
    };

    let creds = platform::rupost_path("credentials.toml");
    let probe = std::env::var("RUPOST_AI_PROBE_URL").ok();
    resolve_with_parsed_and_env(
        &AiRuntimeConfig::from_env(),
        parsed_entry,
        creds.as_deref(),
        probe.as_deref(),
        &|k| std::env::var(k).ok(),
        &sink,
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

    #[tokio::test]
    async fn test_credentials_toml_permission_gate_and_redaction() {
        use crate::diagnostics::BufferedWarningSink;
        let dir = tempfile::tempdir().unwrap();
        let ai_toml = dir.path().join("ai.toml");
        let creds_toml = dir.path().join("credentials.toml");

        std::fs::write(
            &ai_toml,
            r#"
profile = "work"

[profiles.work]
base_url = "https://llm.internal.example/v1"
model = "gpt-4o-mini"
"#,
        )
        .unwrap();

        let secret = "sk-cred-secret-token-987654";
        std::fs::write(
            &creds_toml,
            format!("[profiles.work]\napi_key = \"{secret}\"\n"),
        )
        .unwrap();

        #[cfg(unix)]
        {
            // 1. 0o644 必须被拒绝加载，且通过 WarningSink 提示 chmod 600
            crate::platform::set_unix_mode(&creds_toml, 0o644).unwrap();
            let sink = BufferedWarningSink::new();
            let err = resolve_with_options(
                &env_cfg(None, None, None),
                Some(&ai_toml),
                Some(&creds_toml),
                Some("http://127.0.0.1:1"),
                &sink,
            )
            .await;
            assert!(err.is_err(), "0o644 凭据文件必须拒绝放行");
            let msgs = sink.messages();
            assert!(
                msgs.iter().any(|m| m.contains("chmod 600")),
                "必须提示执行 chmod 600: {msgs:?}"
            );
        }

        // 2. 0o600 放行，且解析到的密钥可被动态脱敏且 Debug 绝不泄漏
        crate::platform::set_unix_mode(&creds_toml, 0o600).unwrap();
        let sink_ok = BufferedWarningSink::new();
        let resolved = resolve_with_options(
            &env_cfg(None, None, None),
            Some(&ai_toml),
            Some(&creds_toml),
            Some("http://127.0.0.1:1"),
            &sink_ok,
        )
        .await
        .expect("0o600 凭据文件应放行");
        assert_eq!(resolved.source, AiSource::UserFile);
        assert_eq!(resolved.resolved_secret.as_deref(), Some(secret));

        let creds_debug = format!(
            "{:?}",
            load_credentials_file_with_sink(&creds_toml, &sink_ok).unwrap()
        );
        assert!(!creds_debug.contains(secret), "Debug 输出严禁包含明文密钥");

        let masked = crate::security::redact::redact_explicit_secrets(
            &format!("Authorization: Bearer {secret}"),
            &[resolved.resolved_secret.unwrap()],
        );
        assert!(!masked.contains(secret));
        assert!(masked.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn test_key_resolution_priority_chain_isolated_from_host_env() {
        use crate::diagnostics::BufferedWarningSink;
        let dir = tempfile::tempdir().unwrap();
        let ai_toml = dir.path().join("ai.toml");
        let creds_toml = dir.path().join("credentials.toml");
        std::fs::write(
            &ai_toml,
            r#"
profile = "deepseek"

[profiles.deepseek]
base_url = "https://api.deepseek.example/v1"
model = "deepseek-chat"
api_key_env = "DEEPSEEK_API_KEY"
"#,
        )
        .unwrap();
        std::fs::write(&creds_toml, "[profiles.deepseek]\napi_key = \"sk-creds\"\n").unwrap();
        crate::platform::set_unix_mode(&creds_toml, 0o600).unwrap();

        let sink = BufferedWarningSink::new();
        let env_none = env_cfg(None, None, None);

        // 1. RUPOST_AI_API_KEY > api_key_env > OPENAI_API_KEY > credentials.toml
        let m1: HashMap<&str, &str> = [
            ("RUPOST_AI_API_KEY", "sk-rupost"),
            ("DEEPSEEK_API_KEY", "sk-deepseek"),
            ("OPENAI_API_KEY", "sk-openai"),
        ]
        .into_iter()
        .collect();
        let r1 = resolve_with_env_lookup(
            &env_none,
            Some(&ai_toml),
            Some(&creds_toml),
            None,
            &|k| m1.get(k).map(|v| (*v).to_string()),
            &sink,
        )
        .await
        .unwrap();
        assert_eq!(r1.resolved_secret.as_deref(), Some("sk-rupost"));

        // 2. api_key_env 胜过 OPENAI_API_KEY（且保留 ai.toml base_url/model）
        let m2: HashMap<&str, &str> = [
            ("DEEPSEEK_API_KEY", "sk-deepseek"),
            ("OPENAI_API_KEY", "sk-openai"),
        ]
        .into_iter()
        .collect();
        let r2 = resolve_with_env_lookup(
            &env_none,
            Some(&ai_toml),
            Some(&creds_toml),
            None,
            &|k| m2.get(k).map(|v| (*v).to_string()),
            &sink,
        )
        .await
        .unwrap();
        assert_eq!(r2.resolved_secret.as_deref(), Some("sk-deepseek"));
        assert_eq!(r2.source, AiSource::UserFile);

        // 3. OPENAI_API_KEY 胜过 credentials.toml
        let m3: HashMap<&str, &str> = [("OPENAI_API_KEY", "sk-openai")].into_iter().collect();
        let r3 = resolve_with_env_lookup(
            &env_none,
            Some(&ai_toml),
            Some(&creds_toml),
            None,
            &|k| m3.get(k).map(|v| (*v).to_string()),
            &sink,
        )
        .await
        .unwrap();
        assert_eq!(r3.resolved_secret.as_deref(), Some("sk-openai"));

        // 4. 无环境变量 → 回退 credentials.toml
        let r4 = resolve_with_env_lookup(
            &env_none,
            Some(&ai_toml),
            Some(&creds_toml),
            None,
            &|_| None,
            &sink,
        )
        .await
        .unwrap();
        assert_eq!(r4.resolved_secret.as_deref(), Some("sk-creds"));
    }

    #[tokio::test]
    async fn test_profile_rejects_plaintext_api_key_and_project_rupost_toml_ai_section() {
        use crate::diagnostics::BufferedWarningSink;
        let dir = tempfile::tempdir().unwrap();
        let ai_toml = dir.path().join("ai.toml");
        std::fs::write(
            &ai_toml,
            "[profiles.work]\nbase_url = \"https://x/v1\"\napi_key = \"sk-leak\"\n",
        )
        .unwrap();

        let sink = BufferedWarningSink::new();
        let res = resolve_with_options(
            &env_cfg(None, None, None),
            Some(&ai_toml),
            None,
            Some("http://127.0.0.1:1"),
            &sink,
        )
        .await;
        assert!(res.is_err());
        assert!(
            sink.messages().iter().any(|m| m.contains("api_key")),
            "profile 明文 api_key 必须触发告警拒载"
        );

        // rupost.toml [ai] 段解析同样遵守 TC-C05
        let rupost_toml = dir.path().join("rupost.toml");
        std::fs::write(
            &rupost_toml,
            "[ai]\nbase_url = \"https://x/v1\"\napi_key = \"sk-leak\"\n",
        )
        .unwrap();
        let proj_err = parse_project_rupost_ai_section(&rupost_toml).unwrap();
        assert!(proj_err.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_credentials_toml_rejects_symlink_even_if_target_is_0600() {
        use crate::diagnostics::{BufferedWarningSink, WarningSeverity};
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.toml");
        std::fs::write(&target, "api_key = \"sk-real\"\n").unwrap();
        crate::platform::set_unix_mode(&target, 0o600).unwrap();

        let link = dir.path().join("credentials.toml");
        symlink(&target, &link).unwrap();

        let sink = BufferedWarningSink::new();
        assert!(load_credentials_file_with_sink(&link, &sink).is_none());
        let recs = sink.records();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].severity, WarningSeverity::Security);
    }

    #[tokio::test]
    async fn test_redacting_ai_client_masks_secret_in_output_and_errors() {
        struct EchoSecretClient(String);
        #[async_trait::async_trait]
        impl AiClient for EchoSecretClient {
            async fn generate_text(&self, _s: &str, _u: &str) -> rupost_ai::Result<String> {
                Err(rupost_ai::AiError::ApiError {
                    status: 401,
                    message: format!("invalid token {}", self.0),
                })
            }
        }
        let secret = "sk-top-secret-42";
        let wrapped =
            wrap_client_with_secret(Arc::new(EchoSecretClient(secret.to_string())), Some(secret));
        let err = wrapped.generate_text("sys", "usr").await.unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains(secret), "错误消息必须掩码 secret: {msg}");
        assert!(msg.contains("[REDACTED]"));
    }
}
