use chrono::{DateTime, Utc};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::history::serialization;

/// 历史记录条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// 唯一 ID (UUID)
    pub id: String,

    /// 请求时间
    pub timestamp: DateTime<Utc>,

    /// 请求耗时 (毫秒)
    pub duration_ms: u64,

    /// 请求快照
    pub request: RequestSnapshot,

    /// 请求来源 (例如 "cli", "file:test.http")
    pub source: Option<String>,

    /// 响应元数据
    pub response: ResponseMeta,
}

/// 请求快照 (用于测试生成)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestSnapshot {
    pub method: String,
    pub url: String,

    #[serde(with = "serialization::header_map")]
    pub headers: HeaderMap,

    pub body: Option<String>,

    /// `@timeout`（仅非流式请求设置）。`serde(default)` 兼容没有该字段的旧快照：
    /// 缺失时重放退回 client 级默认超时，而非报错。
    #[serde(default)]
    pub timeout: Option<std::time::Duration>,
}

/// 响应元数据 (TUI 模式下可选包含 Body，节省空间且向前兼容)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMeta {
    pub status: u16,

    #[serde(with = "serialization::header_map")]
    pub headers: HeaderMap,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// 现场执行错误强类型明细
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionErrorDetail {
    /// 错误分类标签（如 "ConnectionTimeout", "DnsResolutionFailed", "HttpTransport"）
    pub kind: String,
    /// 错误详细描述（必须经过脱敏过滤）
    pub message: String,
    /// 故障发生前的耗时（毫秒）
    pub duration_ms: u64,
    /// 故障发生时间点
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

/// 单个 HTTP 交互快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// 交互唯一 ID
    pub id: String,
    /// 关联的请求快照
    pub request: RequestSnapshot,
    /// 关联的响应快照 (带 Body)；当网络层连接失败或超时时为 None
    #[serde(default)]
    pub response: Option<ResponseSnapshot>,
    /// 现场异常详情（正常收到响应时为 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ExecutionErrorDetail>,
}

impl SnapshotEntry {
    pub fn new_success(id: String, request: RequestSnapshot, response: ResponseSnapshot) -> Self {
        Self {
            id,
            request,
            response: Some(response),
            error: None,
        }
    }

    pub fn new_error(id: String, request: RequestSnapshot, error: ExecutionErrorDetail) -> Self {
        Self {
            id,
            request,
            response: None,
            error: Some(error),
        }
    }
}

/// 快照套件结构
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotSuite {
    /// 快照 Schema 版本号（旧快照未写此字段时默认反序列化为 1）
    #[serde(default = "default_snapshot_version")]
    pub version: u32,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub source_file: Option<String>,
    pub entries: Vec<SnapshotEntry>,
}

fn default_snapshot_version() -> u32 {
    1
}

/// 响应快照 (带 Body)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseSnapshot {
    pub status: u16,

    #[serde(with = "serialization::header_map")]
    pub headers: reqwest::header::HeaderMap,

    pub body: String,
}

impl From<HistoryEntry> for SnapshotEntry {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            id: entry.id,
            request: entry.request,
            response: Some(ResponseSnapshot {
                status: entry.response.status,
                headers: entry.response.headers,
                body: entry
                    .response
                    .body
                    .unwrap_or_else(|| "Old snapshot: body not recorded".to_string()),
            }),
            error: None,
        }
    }
}

impl RequestSnapshot {
    /// 从解析实例化后的 `ParsedRequest` 构建请求历史记录快照
    pub fn from_parsed(parsed: &crate::parser::ParsedRequest) -> Self {
        use reqwest::header::{HeaderName, HeaderValue};
        let mut headers = HeaderMap::new();
        for (k, v) in &parsed.headers {
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(n, v);
            }
        }

        Self {
            method: parsed.method_or_default().to_string(),
            url: parsed.url.clone(),
            headers,
            body: parsed.body.clone(),
            timeout: parsed.metadata.timeout,
        }
    }
}

// =====================================================================
// 离线数据黑匣子契约 (Execution Snapshot, Schema v1.0)
// 唯一权威: doc/plans/2026-11-10-cross-scenario-engine-and-plugins/02 §2
// 演进策略 (02 §2.6): 只允许追加字段、禁止改语义/删字段；消费者必须容忍
// 未知字段；破坏性变更必须递增 schema_version。
// 安全不变量 (02 §2.7): plugin_args 永不序列化进快照；文件落盘 0600。
// =====================================================================

/// 快照契约版本 (02 §2)
pub const SNAPSHOT_SCHEMA_VERSION: &str = "1.0";

/// 单响应体录入上限 (03 §10.6 防 OOM；可通过 `--max-snapshot-body-bytes` 覆盖，M3 接线)
pub const DEFAULT_MAX_SNAPSHOT_BODY_BYTES: usize = 256 * 1024;

/// 文件级脱敏形态标记 (消费者据此判断产物敏感性；掩码模式下验签类
/// 结论 MUST 附带"数据不完整"警告)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaskingMode {
    /// 100% 原始字节保真
    None,
    /// 已应用 --mask-secrets 掩码
    Secrets,
}

/// 运行环境指纹 (OS / 架构 / rupost 版本)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunnerFingerprint {
    pub os: String,
    pub arch: String,
    pub rupost_version: String,
}

/// 三态执行判定 (03 §10.5)：严禁把插件自愈降级混淆为纯粹成功
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    /// 全部插件与断言 100% 正常通过
    Pass,
    /// 发生可选插件缺失/失败降级跳过，但基础 HTTP 请求与断言通过
    DegradedPass,
    /// 断言失败、网络异常或未授权插件缺失
    Fail,
}

/// 离线数据黑匣子文件 (Schema v1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSnapshotFile {
    /// 契约版本，恒定 "1.0"（演进策略见上）
    pub schema_version: String,
    /// 执行时间戳 (ISO 8601 UTC)
    pub generated_at: chrono::DateTime<chrono::Utc>,
    /// 运行环境标识（--env 提供时非空）
    pub environment: Option<String>,
    /// 运行指纹
    pub runner_fingerprint: RunnerFingerprint,
    /// 脱敏形态标记
    pub masking: MaskingMode,
    /// 完整的测试条目列表
    pub entries: Vec<ExecutionSnapshotEntry>,
}

/// 黑匣子单条目：请求/响应/错误/时序/Socket/断言/告警的全量现场
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSnapshotEntry {
    pub request_number: usize,
    pub name: Option<String>,
    pub file_path: String,
    pub success: bool,
    pub outcome: ExecutionOutcome,
    pub status: Option<u16>,
    /// 用例级总耗时（微秒）。口径 = 纯网络总耗时 + 插件耗时（不含断言耗时）；
    /// 插件耗时由 [`ExecutionSnapshotEntry::plugin_duration_us`] 单列承载。
    /// 采集链路缺失时序时回退为执行墙钟（runner duration）。
    pub duration_us: u64,
    /// 外部插件管道总耗时（M3 接线；当前恒为 None）
    #[serde(default)]
    pub plugin_duration_us: Option<u64>,

    /// 原始发包请求镜像（加签与变量解析后的最终形态）
    pub request: SnapshotRequest,

    /// 完整原始响应镜像（256KB 防 OOM 截断 + SHA-256 摘要）。
    /// None 时序列化为 null——核心字段键集合稳定，便于机器消费（对齐
    /// report_schema_test 的键集合稳定哲学）。
    #[serde(default)]
    pub response: Option<SnapshotResponse>,

    /// 网络/协议/插件执行错误详情（故障 100% 录制，绝不静默吞没）
    #[serde(default)]
    pub error: Option<ExecutionErrorDetail>,

    /// 微秒级网络时延拆解（连接复用时对应阶段为 None）
    #[serde(default)]
    pub timing_waterfall: Option<TimingWaterfallUs>,

    /// 底层物理连接诊断指纹
    #[serde(default)]
    pub socket_diagnostics: Option<SocketDiagnostics>,

    /// 规则断言结果明细
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<SnapshotAssertion>,

    /// 运行期非致命告警（降级说明、未定义变量、插件 stderr 摘要等）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// 黑匣子请求镜像（Headers 保序为键值对，区别于 history 的 HeaderMap 无序结构）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// 黑匣子响应镜像（256KB 截断保护 + 截断前完整 Body 的 SHA-256 供离线验真）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// 超过单响应体上限截断时为 true
    pub body_truncated: bool,
    /// 截断前完整原始响应体的 SHA-256 校验和（小写 hex）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_sha256: Option<String>,
}

/// 黑匣子断言明细
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotAssertion {
    pub expression: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 微秒级网络时延拆解。
/// 连接复用语义：None = 该阶段未发生（keep-alive 复用既有连接）；
/// Some(0) 仅在阶段实际发生且耗时向下取整为 0 时出现。
/// HTTP/2 多路复用下 socket_diagnostics 描述共享连接而非单请求独占。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimingWaterfallUs {
    pub dns_lookup_us: Option<u64>,
    pub tcp_connect_us: Option<u64>,
    pub tls_handshake_us: Option<u64>,
    pub ttfb_us: u64,
    pub transfer_us: u64,
    /// 纯网络侧总耗时 = 已发生各阶段之和；与 duration_us 的差值 = 插件耗时
    /// （由 plugin_duration_us 单列承载，M3 接线）
    pub total_us: u64,
}

/// 底层物理连接诊断指纹（证书字段 best-effort，未采集时为 None，绝不阻塞主流程）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SocketDiagnostics {
    pub remote_ip: String,
    pub remote_port: u16,
    pub http_version: String,
    pub tls_cipher_suite: Option<String>,
    pub tls_version: Option<String>,
    pub cert_days_remaining: Option<i64>,
    pub cert_issuer: Option<String>,
}

impl TimingWaterfallUs {
    /// 从 runner 采集的细粒度时序 + 深度诊断报告合成微秒级瀑布。
    /// TLS 阶段仅在 diagnose 报告可得时填充（reqwest 公开 API 不暴露握手时序）。
    pub fn from_probes(
        timing: &crate::http::timing::RequestTiming,
        diagnose: Option<&crate::http::diagnose::DiagnosticsReport>,
    ) -> Self {
        let dns_us = timing.dns_lookup.as_micros() as u64;
        let tcp_us = timing.tcp_connect.as_micros() as u64;
        let tls = diagnose.and_then(|d| d.tls_handshake_duration.map(|t| t.as_micros() as u64));
        let ttfb = timing.ttfb.as_micros() as u64;
        let transfer = timing.transfer.as_micros() as u64;
        let total = dns_us + tcp_us + tls.unwrap_or(0) + ttfb + transfer;
        Self {
            dns_lookup_us: Some(dns_us),
            tcp_connect_us: Some(tcp_us),
            tls_handshake_us: tls,
            ttfb_us: ttfb,
            transfer_us: transfer,
            total_us: total,
        }
    }
}

impl SocketDiagnostics {
    /// 从深度诊断报告提取物理连接指纹；非 HTTPS 时 TLS/证书字段为 None
    pub fn from_diagnose(diagnose: &crate::http::diagnose::DiagnosticsReport) -> Option<Self> {
        let port = url::Url::parse(&diagnose.url)
            .ok()
            .map(|u| u.port_or_known_default().unwrap_or(80))?;
        let cert = diagnose.cert_info.as_ref().filter(|_| diagnose.is_https);
        Some(Self {
            remote_ip: diagnose.resolved_ips.first().cloned().unwrap_or_default(),
            remote_port: port,
            http_version: diagnose
                .http_version
                .clone()
                .unwrap_or_else(|| "HTTP/1.1".to_string()),
            tls_cipher_suite: None, // CertInfo 未采集 cipher suite（best-effort）
            tls_version: None,
            cert_days_remaining: cert.map(|c| c.days_remaining),
            cert_issuer: cert.map(|c| c.issuer.clone()),
        })
    }
}

fn header_pairs_to_map(pairs: &[(String, String)]) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderName, HeaderValue};
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            map.append(n, v);
        }
    }
    map
}

impl SnapshotRequest {
    /// 黑匣子请求镜像 → 重放格式的 RequestSnapshot（timeout 不可考，回退 None）
    pub fn to_history_request(&self) -> RequestSnapshot {
        RequestSnapshot {
            method: self.method.clone(),
            url: self.url.clone(),
            headers: header_pairs_to_map(&self.headers),
            body: self.body.clone(),
            timeout: None,
        }
    }
}

impl ExecutionSnapshotFile {
    /// 黑匣子契约快照 → 重放套件（`replay` 消费 `--save-snapshot` 产物的桥，
    /// 02 §3.2：现场 dump 拷回开发机后可直接 `replay --explain`）
    pub fn to_replay_suite(&self) -> SnapshotSuite {
        SnapshotSuite {
            version: 2,
            timestamp: self.generated_at,
            source_file: None,
            entries: self
                .entries
                .iter()
                .map(|e| SnapshotEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    request: e.request.to_history_request(),
                    response: e.response.as_ref().map(|r| ResponseSnapshot {
                        status: r.status,
                        headers: header_pairs_to_map(&r.headers),
                        body: r.body.clone(),
                    }),
                    error: e.error.clone(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedRequest;

    #[test]
    fn test_request_snapshot_from_parsed() {
        let mut parsed = ParsedRequest::new(1);
        parsed.method = Some("POST".to_string());
        parsed.url = "http://example.com".to_string();
        parsed.headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        parsed.body = Some("{}".to_string());

        let snapshot = RequestSnapshot::from_parsed(&parsed);
        assert_eq!(snapshot.method, "POST");
        assert_eq!(snapshot.url, "http://example.com");
        assert_eq!(
            snapshot
                .headers
                .get("Content-Type")
                .unwrap()
                .to_str()
                .unwrap(),
            "application/json"
        );
        assert_eq!(snapshot.body, Some("{}".to_string()));
    }

    #[test]
    fn test_request_snapshot_timeout_roundtrip_and_old_format_compat() {
        // 回归：快照此前不记录 @timeout——重放带超时语义的请求会静默退回 30s 默认
        let mut parsed = ParsedRequest::new(1);
        parsed.metadata.timeout = Some(std::time::Duration::from_millis(500));
        let snapshot = RequestSnapshot::from_parsed(&parsed);
        assert_eq!(
            snapshot.timeout,
            Some(std::time::Duration::from_millis(500))
        );

        // 序列化往返保持 timeout
        let json = serde_json::to_string(&snapshot).unwrap();
        let back: RequestSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.timeout, Some(std::time::Duration::from_millis(500)));

        // 旧格式快照（无 timeout 字段）反序列化为 None，不报错
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value.as_object_mut().unwrap().remove("timeout");
        let old: RequestSnapshot = serde_json::from_value(value).unwrap();
        assert_eq!(old.timeout, None);
    }
}
