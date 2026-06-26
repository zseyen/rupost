use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use url::Url;

/// 细粒度网络时序诊断结构体
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RequestTiming {
    pub dns_lookup: Duration,  // DNS 解析耗时
    pub tcp_connect: Duration, // TCP 连接建立耗时
    pub ttfb: Duration,        // 首字节响应 (Wait) 耗时
    pub transfer: Duration,    // 数据传输 (Receive) 耗时
}

/// 基于第一性原理的 Pre-flight TCP & DNS 探测诊断工具
pub struct DiagnosticsProber;

impl DiagnosticsProber {
    /// 诊断给定 URL 的 DNS 和 TCP 握手耗时
    pub async fn probe_connection(url_str: &str) -> Result<(Duration, Duration), String> {
        let url = Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;
        let host = url
            .host_str()
            .ok_or_else(|| "Missing host in URL".to_string())?;

        // 提取端口或默认端口
        let port = url.port_or_known_default().unwrap_or(80);

        // 1. 测量 DNS 解析耗时
        let dns_start = Instant::now();
        let addrs = tokio::net::lookup_host(format!("{}:{}", host, port))
            .await
            .map_err(|e| format!("DNS lookup failed: {}", e))?;
        let dns_lookup = dns_start.elapsed();

        let ip_addr = addrs
            .into_iter()
            .next()
            .ok_or_else(|| "No IP addresses found".to_string())?;

        // 2. 测量 TCP 握手建立耗时
        let tcp_start = Instant::now();
        let _stream = TcpStream::connect(ip_addr)
            .await
            .map_err(|e| format!("TCP connection failed: {}", e))?;
        let tcp_connect = tcp_start.elapsed();

        // 探测完成后，连接自动释放
        Ok((dns_lookup, tcp_connect))
    }

    /// 根据诊断探测结果和实际请求耗时解析并生成 RequestTiming 结构
    pub fn resolve_timing(
        probe_result: Option<(Duration, Duration)>,
        ttfb: Duration,
        transfer: Duration,
        need_timing: bool,
    ) -> Option<RequestTiming> {
        if need_timing {
            if let Some((dns, tcp)) = probe_result {
                return Some(RequestTiming {
                    dns_lookup: dns,
                    tcp_connect: tcp,
                    ttfb,
                    transfer,
                });
            }
        }
        None
    }
}
