use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::net::{TcpStream, lookup_host};
use tokio::time::timeout;
use url::Url;

/// 细粒度网络时序诊断结构体
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RequestTiming {
    pub dns_lookup: Duration,  // DNS 解析耗时
    pub tcp_connect: Duration, // TCP 连接建立耗时
    pub ttfb: Duration,        // 首字节响应 (Wait) 耗时
    pub transfer: Duration,    // 数据传输 (Receive) 耗时
}

/// 统一时延结构
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct NetworkLatency {
    pub dns_lookup: Duration,
    pub tcp_connect: Duration,
    pub tls_handshake: Option<Duration>,
    pub ttfb: Option<Duration>,
    pub transfer: Option<Duration>,
    pub total: Duration,
}

/// 基于第一性原理的 Pre-flight TCP & DNS 探测诊断工具
pub struct DiagnosticsProber;

impl DiagnosticsProber {
    /// 统一的 DNS 解析与 TCP 建连测量工具，带细粒度超时控制
    pub async fn connect_tcp_with_timeout(
        host: &str,
        port: u16,
        timeout_dur: Duration,
    ) -> Result<(TcpStream, std::net::SocketAddr, Duration, Duration), String> {
        // 1. 测量 DNS 解析耗时 (带有超时限制)
        let dns_start = Instant::now();
        let addrs_iter = match timeout(timeout_dur, lookup_host(format!("{}:{}", host, port))).await
        {
            Ok(Ok(addrs)) => addrs,
            Ok(Err(e)) => return Err(format!("DNS lookup failed: {}", e)),
            Err(_) => return Err(format!("DNS lookup timeout after {:?}", timeout_dur)),
        };
        let dns_lookup = dns_start.elapsed();

        let addrs_vec: Vec<std::net::SocketAddr> = addrs_iter.collect();
        let target_addr = *addrs_vec
            .first()
            .ok_or_else(|| "No IP addresses resolved".to_string())?;

        // 2. 测量 TCP 握手建立耗时 (带有超时限制)
        let tcp_start = Instant::now();
        let stream = match timeout(timeout_dur, TcpStream::connect(target_addr)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => return Err(format!("TCP connection failed: {}", e)),
            Err(_) => return Err(format!("TCP connection timeout after {:?}", timeout_dur)),
        };
        let tcp_connect = tcp_start.elapsed();

        Ok((stream, target_addr, dns_lookup, tcp_connect))
    }

    /// 诊断给定 URL 的 DNS 和 TCP 握手耗时
    pub async fn probe_connection(url_str: &str) -> Result<(Duration, Duration), String> {
        let url = Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;
        let host = url
            .host_str()
            .ok_or_else(|| "Missing host in URL".to_string())?;

        // 提取端口或默认端口
        let port = url.port_or_known_default().unwrap_or(80);

        // 复用统一的超时建连逻辑 (默认 5 秒超时)
        let default_timeout = Duration::from_secs(5);
        let (_, _, dns_lookup, tcp_connect) =
            Self::connect_tcp_with_timeout(host, port, default_timeout).await?;

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
        if need_timing && let Some((dns, tcp)) = probe_result {
            return Some(RequestTiming {
                dns_lookup: dns,
                tcp_connect: tcp,
                ttfb,
                transfer,
            });
        }
        None
    }
}
