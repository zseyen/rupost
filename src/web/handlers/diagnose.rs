use crate::http::diagnose::diagnose_url;
use crate::web::server::AppState;
use crate::web::types::{ApiResponse, DiagnoseRequest};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::Url;

/// 判断 IP 是否属于内网、回环、链路本地（如云元数据 169.254.169.254）或多播/未指定地址
fn is_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_restricted_ipv4(v4),
        IpAddr::V6(v6) => is_restricted_ipv6(v6),
    }
}

fn is_restricted_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.octets()[0] == 0
        // 100.64.0.0/10 (CGNAT)
        || (ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000) == 64)
}

fn is_restricted_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_restricted_ipv4(v4);
    }
    ip.is_loopback()
        || ip.is_multicast()
        || ip.is_unspecified()
        || (ip.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 ULA
        || (ip.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
}

/// SSRF 护栏（SSOT：diagnose 与 ws_bridge 共用）：
/// 1. 仅放行指定协议（http/https 或 ws/wss）
/// 2. 若主机不在 allowed_hosts 白名单中，拦截内网、回环、云元数据与私有后缀
pub async fn validate_outbound_url(
    raw_url: &str,
    allowed_schemes: &[&str],
    allowed_hosts: &[String],
) -> std::result::Result<Url, String> {
    let parsed = Url::parse(raw_url.trim()).map_err(|e| format!("Invalid URL: {}", e))?;
    if !allowed_schemes.contains(&parsed.scheme()) {
        return Err(format!(
            "Unsupported URL scheme '{}': expected one of {:?}",
            parsed.scheme(),
            allowed_schemes
        ));
    }

    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "URL must include a valid host".to_string())?;
    let host_clean = host_raw.trim_start_matches('[').trim_end_matches(']');

    let explicitly_allowed = allowed_hosts
        .iter()
        .any(|h| h.eq_ignore_ascii_case(host_clean) || h.eq_ignore_ascii_case(host_raw));
    if explicitly_allowed {
        return Ok(parsed);
    }

    let lower = host_clean.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".local") || lower.ends_with(".internal") {
        return Err(format!(
            "Blocked by SSRF guard: restricted host '{}'",
            host_clean
        ));
    }

    if let Ok(ip) = host_clean.parse::<IpAddr>() {
        if is_restricted_ip(ip) {
            return Err(format!(
                "Blocked by SSRF guard: restricted IP '{}'",
                host_clean
            ));
        }
        return Ok(parsed);
    }

    // DNS 解析预检：若解析结果包含受限 IP，阻断 DNS Rebinding
    if let Some(port) = parsed.port_or_known_default()
        && let Ok(addrs) = tokio::net::lookup_host((host_clean, port)).await
    {
        for addr in addrs {
            if is_restricted_ip(addr.ip()) {
                return Err(format!(
                    "Blocked by SSRF guard: '{}' resolved to restricted IP {}",
                    host_clean,
                    addr.ip()
                ));
            }
        }
    }

    Ok(parsed)
}

/// 链路诊断接口（只读模式禁止，且受 SSRF 护栏保护）
pub async fn diagnose_handler(
    State(state): State<AppState>,
    Json(req): Json<DiagnoseRequest>,
) -> impl IntoResponse {
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::err("Diagnose is disabled in read-only mode")),
        );
    }

    if let Err(reason) = validate_outbound_url(
        &req.url,
        &["http", "https", "ws", "wss"],
        &state.allowed_hosts,
    )
    .await
    {
        return (StatusCode::FORBIDDEN, Json(ApiResponse::err(reason)));
    }

    match diagnose_url(&req.url).await {
        Ok(report) => (StatusCode::OK, Json(ApiResponse::ok(report))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::err(format!("Diagnose failed: {}", e))),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ssrf_guard_blocks_private_and_metadata_ips() {
        for blocked in [
            "http://169.254.169.254/latest/meta-data/",
            "http://127.0.0.1:8080/admin",
            "http://10.0.0.1/internal",
            "http://172.16.0.1/internal",
            "http://192.168.1.1/router",
            "http://[::1]:8080/",
            "http://localhost:3000/",
            "file:///etc/passwd",
        ] {
            assert!(
                validate_outbound_url(blocked, &["http", "https"], &[])
                    .await
                    .is_err(),
                "expected SSRF guard to block {blocked}"
            );
        }

        // 显式加入 allowed_hosts 时放行
        assert!(
            validate_outbound_url(
                "http://127.0.0.1:8080/health",
                &["http", "https"],
                &["127.0.0.1".to_string()],
            )
            .await
            .is_ok()
        );
    }
}
