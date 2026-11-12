#![allow(clippy::clone_on_copy, clippy::collapsible_if)]
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

/// DNS/TCP 分阶段探测产物：连接失败不提前返回，保留已测得的阶段性数据
/// （DNS 成功 + TCP 失败时 `socket_addr` 仍可用），供连接层错误拆分与
/// [`crate::http::diagnose_url_lenient`] 的部分诊断报告使用。
pub struct StagedTcpProbe {
    pub stream: Option<TcpStream>,
    pub socket_addr: Option<std::net::SocketAddr>,
    pub dns_lookup_duration: Duration,
    pub tcp_connect_duration: Option<Duration>,
    /// 首个失败阶段的分类信息（"DNS lookup failed: ..." / "TCP connection failed: ..."）
    pub failure: Option<String>,
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
        let probe = Self::connect_tcp_staged(host, port, timeout_dur).await;
        match (probe.stream, probe.socket_addr, probe.tcp_connect_duration) {
            (Some(stream), Some(addr), Some(tcp)) => {
                Ok((stream, addr, probe.dns_lookup_duration, tcp))
            }
            _ => Err(probe
                .failure
                .unwrap_or_else(|| "connection failed".to_string())),
        }
    }

    /// 分阶段建连探测（[`connect_tcp_with_timeout`] 的无早退变体）：
    /// DNS 或任一阶段失败时仍返回已完成的阶段数据与失败分类，不吞掉 Err 语义
    /// 由调用方决定（严格路径取 `failure` 转 Err，宽容路径转部分诊断报告）。
    pub async fn connect_tcp_staged(
        host: &str,
        port: u16,
        timeout_dur: Duration,
    ) -> StagedTcpProbe {
        // 1. 测量 DNS 解析耗时 (带有超时限制)
        let dns_start = Instant::now();
        let addrs_iter = match timeout(timeout_dur, lookup_host(format!("{}:{}", host, port))).await
        {
            Ok(Ok(addrs)) => addrs,
            Ok(Err(e)) => {
                return StagedTcpProbe {
                    stream: None,
                    socket_addr: None,
                    dns_lookup_duration: dns_start.elapsed(),
                    tcp_connect_duration: None,
                    failure: Some(format!("DNS lookup failed: {}", e)),
                };
            }
            Err(_) => {
                return StagedTcpProbe {
                    stream: None,
                    socket_addr: None,
                    dns_lookup_duration: dns_start.elapsed(),
                    tcp_connect_duration: None,
                    failure: Some(format!("DNS lookup timeout after {:?}", timeout_dur)),
                };
            }
        };
        let dns_lookup = dns_start.elapsed();

        let addrs_vec: Vec<std::net::SocketAddr> = addrs_iter.collect();
        let Some(target_addr) = addrs_vec.first().copied() else {
            return StagedTcpProbe {
                stream: None,
                socket_addr: None,
                dns_lookup_duration: dns_lookup,
                tcp_connect_duration: None,
                failure: Some("No IP addresses resolved".to_string()),
            };
        };

        // 2. 测量 TCP 握手建立耗时 (带有超时限制)；失败时保留已解析的 IP
        //    与实际尝试耗时，使部分诊断能区分"DNS 坏"与"TCP 坏"
        let tcp_start = Instant::now();
        let stream = match timeout(timeout_dur, TcpStream::connect(target_addr)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => {
                return StagedTcpProbe {
                    stream: None,
                    socket_addr: Some(target_addr),
                    dns_lookup_duration: dns_lookup,
                    tcp_connect_duration: Some(tcp_start.elapsed()),
                    failure: Some(format!("TCP connection failed: {}", e)),
                };
            }
            Err(_) => {
                return StagedTcpProbe {
                    stream: None,
                    socket_addr: Some(target_addr),
                    dns_lookup_duration: dns_lookup,
                    tcp_connect_duration: Some(tcp_start.elapsed()),
                    failure: Some(format!("TCP connection timeout after {:?}", timeout_dur)),
                };
            }
        };
        let tcp_connect = tcp_start.elapsed();

        StagedTcpProbe {
            stream: Some(stream),
            socket_addr: Some(target_addr),
            dns_lookup_duration: dns_lookup,
            tcp_connect_duration: Some(tcp_connect),
            failure: None,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 占坑后释放的回环端口：立即连接必然被拒绝（与 debug_on_failure_test 同款）
    fn closed_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    #[tokio::test]
    async fn test_staged_probe_tcp_failure_preserves_dns_stage() {
        // Windows 回环连接未监听端口会经过 ~2s SYN 重传才返回 WSAECONNREFUSED，
        // 若超时仅设 2s 会先触发 timeout；放宽至 10s 确保跨平台稳定落入 ConnectionRefused。
        let probe = DiagnosticsProber::connect_tcp_staged(
            "127.0.0.1",
            closed_port(),
            Duration::from_secs(10),
        )
        .await;
        assert!(probe.stream.is_none(), "连接失败不得产出流");
        assert!(
            probe
                .failure
                .as_deref()
                .unwrap_or("")
                .starts_with("TCP connection failed"),
            "失败分类应落在 TCP 阶段: {:?}",
            probe.failure
        );
        assert!(
            probe.socket_addr.is_some(),
            "DNS 已成功，解析出的 IP 必须保留（区别于 DNS 失败场景）"
        );
        assert!(
            probe.tcp_connect_duration.is_some(),
            "失败尝试的实际耗时也应保留供瀑布图展示"
        );
    }

    #[tokio::test]
    async fn test_connect_tcp_with_timeout_delegates_failure_classification() {
        // 委托层契约不变：失败仍为 Err 且错误文案与既有 diagnose 命令一致
        let err = DiagnosticsProber::connect_tcp_with_timeout(
            "127.0.0.1",
            closed_port(),
            Duration::from_secs(10),
        )
        .await
        .unwrap_err();
        assert!(
            err.starts_with("TCP connection failed"),
            "错误分类文案不得漂移: {err}"
        );
    }
}
