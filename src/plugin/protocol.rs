//! Stdio IPC 协议契约、失败分类学与单 JSON 补丁解析（03 §8/§10 SSOT）。

use super::PluginPatch;

/// 操作系统 Stdio IPC 管道硬限制载荷阈值 (10MB，防止打满管道缓冲区挂死)
pub const MAX_PLUGIN_PAYLOAD_BYTES: usize = 10 * 1024 * 1024;

/// 插件调用失败分类（03 §8.3 唯一权威；与"缺失"共享降级开关）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginFailureKind {
    /// 超过超时窗，进程组已被 SIGKILL
    Timeout,
    /// stdout 非单 JSON / 版本不兼容 / 载荷超限 / 失败信封
    InvalidOutput(String),
    /// 子进程被信号终止
    Crashed(String),
    /// exit ≠ 0 且无合法输出
    ExitNonZero(i32, String),
}

impl PluginFailureKind {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::InvalidOutput(_) => "invalid_output",
            Self::Crashed(_) => "crashed",
            Self::ExitNonZero(_, _) => "exit_non_zero",
        }
    }

    /// 人类可读摘要（含 stderr 摘要/信号编号，落报告与快照 warnings）
    pub fn describe(&self) -> String {
        match self {
            Self::Timeout => "插件执行超时（进程组已熔断）".to_string(),
            Self::InvalidOutput(detail) => format!("插件输出非法（{detail}）"),
            Self::Crashed(signal) => format!("插件进程被信号终止（{signal}）"),
            Self::ExitNonZero(code, stderr) => {
                let brief: String = stderr.chars().take(200).collect();
                format!("插件退出码 {code}：{brief}")
            }
        }
    }
}

/// 一次插件调用的完整结果
#[derive(Debug, Clone)]
pub struct PluginInvocationOutcome {
    /// 成功解析的 stdout JSON（单对象），失败时为 None
    pub output: Option<serde_json::Value>,
    pub failure: Option<PluginFailureKind>,
    /// 插件耗时（微秒）——时延隔离：只进 plugin_duration_us，不进 TTFB/duration
    pub duration_us: u64,
    /// stderr 摘要（截断 16KB；CI 模式不直通终端）
    pub stderr_tail: String,
}

/// stdout 单对象纪律（03 §8.2）：非单 JSON / 版本不兼容 / 失败信封 → `InvalidOutput`
pub fn parse_plugin_output(
    stdout_bytes: &[u8],
) -> std::result::Result<serde_json::Value, PluginFailureKind> {
    let text = String::from_utf8_lossy(stdout_bytes);
    let trimmed = text.trim();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Err(PluginFailureKind::InvalidOutput(
            "stdout 不是单个 JSON 对象（插件日志必须走 stderr）".to_string(),
        ));
    };
    if !value.is_object() {
        return Err(PluginFailureKind::InvalidOutput(
            "stdout 顶层必须是 JSON 对象".to_string(),
        ));
    }
    if let Some(ok) = value.get("ok").and_then(|v| v.as_bool())
        && !ok
    {
        let message = value
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unspecified");
        return Err(PluginFailureKind::InvalidOutput(format!(
            "插件失败信封: {message}"
        )));
    }
    if let Some(version) = value.get("protocol_version").and_then(|v| v.as_str())
        && !version.starts_with("1.")
    {
        return Err(PluginFailureKind::InvalidOutput(format!(
            "不兼容的协议版本: {version}"
        )));
    }
    Ok(value)
}

/// 从已校验的插件输出对象提取强类型 `PluginPatch`
pub fn parse_patch_from_output(output: &serde_json::Value) -> PluginPatch {
    PluginPatch {
        extra_headers: output
            .get("extra_headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        remove_headers: output
            .get("remove_headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        mutated_body: output.get("mutated_body").and_then(|v| {
            if v.is_null() {
                None
            } else {
                v.as_str().map(|s| s.to_string())
            }
        }),
        set_variables: output
            .get("set_variables")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        warnings: output
            .get("warnings")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plugin_output_rejects_bad_payloads() {
        assert!(matches!(
            parse_plugin_output(b"debug log\n{\"protocol_version\":\"1.0\"}"),
            Err(PluginFailureKind::InvalidOutput(_))
        ));
        assert!(matches!(
            parse_plugin_output(b"[1,2]"),
            Err(PluginFailureKind::InvalidOutput(_))
        ));
        assert!(matches!(
            parse_plugin_output(b"{\"protocol_version\":\"2.0\"}"),
            Err(PluginFailureKind::InvalidOutput(msg)) if msg.contains("2.0")
        ));
        assert!(matches!(
            parse_plugin_output(b"{\"ok\":false,\"message\":\"bad key\"}"),
            Err(PluginFailureKind::InvalidOutput(msg)) if msg.contains("bad key")
        ));
    }

    #[test]
    fn test_parse_patch_from_output_extracts_fields() {
        let val = serde_json::json!({
            "protocol_version": "1.0",
            "extra_headers": [["X-A", "1"]],
            "remove_headers": ["X-Old"],
            "mutated_body": "new-body",
            "set_variables": {"K": "V"},
            "warnings": ["w1"]
        });
        let patch = parse_patch_from_output(&val);
        assert_eq!(patch.extra_headers, vec![("X-A".into(), "1".into())]);
        assert_eq!(patch.remove_headers, vec!["X-Old"]);
        assert_eq!(patch.mutated_body.as_deref(), Some("new-body"));
        assert_eq!(patch.set_variables.get("K").unwrap(), "V");
        assert_eq!(patch.warnings, vec!["w1"]);
    }
}
