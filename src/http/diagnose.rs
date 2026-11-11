use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use url::Url;
use x509_parser::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CertInfo {
    pub subject: String,
    pub issuer: String,
    pub sans: Vec<String>,
    pub validity_not_after: String,
    pub days_remaining: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsReport {
    pub url: String,
    pub is_https: bool,
    pub is_websocket: bool,
    pub ws_upgrade_success: Option<bool>,
    pub resolved_ips: Vec<String>,
    pub dns_lookup_duration: Duration,
    pub tcp_connect_duration: Duration,
    pub tls_handshake_duration: Option<Duration>,
    pub cert_info: Option<CertInfo>,
    pub http_status: Option<u16>,
    pub http_version: Option<String>,
    pub ttfb: Option<Duration>,
    pub total_duration: Duration,
    /// 连接阶段（DNS/TCP）首个失败的分类信息（"DNS lookup failed: ..." /
    /// "TCP connection failed: ..."）；连接成功时为 None。始终序列化（None → null），
    /// 保证成功/失败形态键集合一致（tests/report_schema_test.rs 锁定）。
    #[serde(default)]
    pub connection_error: Option<String>,
}

/// 解析 X509 证书的 DER 裸字节
pub fn parse_x509_der(der: &[u8]) -> Result<CertInfo, String> {
    let (_, x509) =
        parse_x509_certificate(der).map_err(|e| format!("X509 parse error: {:?}", e))?;

    let subject = x509.subject().to_string();
    let issuer = x509.issuer().to_string();

    let not_after = x509.validity().not_after;
    let not_after_timestamp = not_after.timestamp();
    let validity_not_after =
        chrono::DateTime::from_timestamp(not_after_timestamp, 0).unwrap_or_else(chrono::Utc::now);

    let now = chrono::Utc::now();
    let days_remaining = validity_not_after.signed_duration_since(now).num_days();

    // 提取 SANs (Subject Alternative Names)
    let mut sans = Vec::new();
    for extension in x509.extensions() {
        if let ParsedExtension::SubjectAlternativeName(san) = extension.parsed_extension() {
            for name in &san.general_names {
                match name {
                    GeneralName::DNSName(dns) => sans.push(dns.to_string()),
                    GeneralName::IPAddress(ip) => sans.push(format!("{:?}", ip)),
                    _ => {}
                }
            }
        }
    }

    Ok(CertInfo {
        subject,
        issuer,
        sans,
        validity_not_after: validity_not_after.to_rfc3339(),
        days_remaining,
    })
}

fn parse_http_response_header(bytes: &[u8]) -> Option<(u16, String)> {
    let s = std::str::from_utf8(bytes).ok()?;
    let first_line = s.lines().next()?;
    // 类似于 "HTTP/1.1 200 OK"
    let mut parts = first_line.split_whitespace();
    let version = parts.next()?.to_string();
    let status_code_str = parts.next()?;
    let status_code = status_code_str.parse::<u16>().ok()?;
    Some((status_code, version))
}

/// 诊断目标的解析产物（严格/宽容两条诊断路径共用的 URL 语义单一来源）
struct DiagnoseTarget {
    host: String,
    path: String,
    port: u16,
    is_https: bool,
    is_websocket: bool,
}

/// 各诊断阶段（DNS/TCP/TLS/TTFB）共用的统一超时
const DIAGNOSE_STAGE_TIMEOUT: Duration = Duration::from_secs(5);

fn resolve_target(url_str: &str) -> Result<DiagnoseTarget, String> {
    let url = Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;
    let host = url
        .host_str()
        .ok_or_else(|| "Missing host in URL".to_string())?
        .to_string();

    // 协议识别
    let orig_scheme = url.scheme().to_lowercase();
    let is_websocket = orig_scheme == "ws" || orig_scheme == "wss";
    let is_https = orig_scheme == "https" || orig_scheme == "wss";

    // 解析端口或默认端口
    let port = url
        .port_or_known_default()
        .unwrap_or(if is_https { 443 } else { 80 });

    Ok(DiagnoseTarget {
        host,
        path: url.path().to_string(),
        port,
        is_https,
        is_websocket,
    })
}

/// 连接建立后的共尾部：TLS 握手 + HTTP TTFB 探测与报告组装。
/// lenient = false 时遵循严格契约：TLS/读写阶段失败直接返回 Err；
/// lenient = true 时遵循宽容契约：保留已测得的 TLS 耗时与证书实体，将阶段失败下沉至 connection_error。
#[allow(unused_assignments, clippy::too_many_arguments)]
async fn probe_after_connect(
    tcp_stream: tokio::net::TcpStream,
    socket_addr: std::net::SocketAddr,
    url_str: &str,
    target: &DiagnoseTarget,
    start_all: Instant,
    dns_lookup_duration: Duration,
    tcp_connect_duration: Duration,
    lenient: bool,
) -> Result<DiagnosticsReport, String> {
    let host = target.host.as_str();
    let is_https = target.is_https;
    let is_websocket = target.is_websocket;
    let default_timeout = DIAGNOSE_STAGE_TIMEOUT;
    let resolved_ips = vec![socket_addr.ip().to_string()];

    let mut tls_handshake_duration = None;
    let mut cert_info = None;
    let mut http_status = None;
    let mut http_version = None;
    let mut ttfb = None;

    let handle_stage_error = |err: String,
                              tls_dur: Option<Duration>,
                              cert: Option<CertInfo>|
     -> Result<DiagnosticsReport, String> {
        if lenient {
            Ok(DiagnosticsReport {
                url: url_str.to_string(),
                is_https,
                is_websocket,
                ws_upgrade_success: None,
                resolved_ips: resolved_ips.clone(),
                dns_lookup_duration,
                tcp_connect_duration,
                tls_handshake_duration: tls_dur,
                cert_info: cert,
                http_status: None,
                http_version: None,
                ttfb: None,
                total_duration: start_all.elapsed(),
                connection_error: Some(err),
            })
        } else {
            Err(err)
        }
    };

    // 根据是否是 WebSocket，自适应组装 Upgrade 握手或常规 HTTP 请求
    let request_payload = if is_websocket {
        format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n",
            target.path, host
        )
    } else {
        format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            target.path, host
        )
    };

    if is_https {
        // 3. TLS 握手诊断
        let mut root_store = RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let config = ClientConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("Failed to set TLS protocol versions: {:?}", e))?
        .with_root_certificates(root_store)
        .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(config));
        let server_name = ServerName::try_from(host.to_string())
            .map_err(|e| format!("Invalid server name '{}': {}", host, e))?;

        let tls_start = Instant::now();
        let mut tls_stream =
            match tokio::time::timeout(default_timeout, connector.connect(server_name, tcp_stream))
                .await
            {
                Ok(Ok(stream)) => stream,
                Ok(Err(e)) => {
                    return handle_stage_error(format!("TLS handshake failed: {}", e), None, None);
                }
                Err(_) => {
                    return handle_stage_error(
                        format!("TLS handshake timeout after {:?}", default_timeout),
                        None,
                        None,
                    );
                }
            };
        tls_handshake_duration = Some(tls_start.elapsed());

        // 获取对端证书链
        let (_, connection) = tls_stream.get_ref();
        if let Some(certs) = connection.peer_certificates()
            && let Some(first_cert) = certs.first()
        {
            cert_info = parse_x509_der(first_cert.as_ref()).ok();
        }

        // 4. HTTP TTFB (HTTPS)
        let ttfb_start = Instant::now();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        if let Err(e) = tls_stream.write_all(request_payload.as_bytes()).await {
            return handle_stage_error(
                format!("Failed to send request over TLS: {}", e),
                tls_handshake_duration,
                cert_info,
            );
        }
        tls_stream.flush().await.ok();

        let mut buffer = [0u8; 1024];
        let n = match tokio::time::timeout(default_timeout, tls_stream.read(&mut buffer)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                return handle_stage_error(
                    format!("Failed to read response over TLS: {}", e),
                    tls_handshake_duration,
                    cert_info,
                );
            }
            Err(_) => {
                return handle_stage_error(
                    format!("HTTP TTFB timeout after {:?}", default_timeout),
                    tls_handshake_duration,
                    cert_info,
                );
            }
        };

        if n == 0 {
            return handle_stage_error(
                "Connection closed by server over TLS before HTTP response was received (EOF)"
                    .to_string(),
                tls_handshake_duration,
                cert_info,
            );
        }

        ttfb = Some(ttfb_start.elapsed());

        if let Some((status, version)) = parse_http_response_header(&buffer[..n]) {
            http_status = Some(status);
            http_version = Some(version);
        } else {
            return handle_stage_error(
                "Invalid HTTP response header over TLS from server".to_string(),
                tls_handshake_duration,
                cert_info,
            );
        }
    } else {
        // 4. HTTP TTFB (HTTP)
        let ttfb_start = Instant::now();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tcp_stream;
        if let Err(e) = stream.write_all(request_payload.as_bytes()).await {
            return handle_stage_error(format!("Failed to send request: {}", e), None, None);
        }
        stream.flush().await.ok();

        let mut buffer = [0u8; 1024];
        let n = match tokio::time::timeout(default_timeout, stream.read(&mut buffer)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                return handle_stage_error(format!("Failed to read response: {}", e), None, None);
            }
            Err(_) => {
                return handle_stage_error(
                    format!("HTTP TTFB timeout after {:?}", default_timeout),
                    None,
                    None,
                );
            }
        };

        if n == 0 {
            return handle_stage_error(
                "Connection closed by server before HTTP response was received (EOF)".to_string(),
                None,
                None,
            );
        }

        ttfb = Some(ttfb_start.elapsed());

        if let Some((status, version)) = parse_http_response_header(&buffer[..n]) {
            http_status = Some(status);
            http_version = Some(version);
        } else {
            return handle_stage_error(
                "Invalid HTTP response header from server".to_string(),
                None,
                None,
            );
        }
    }

    let total_duration = start_all.elapsed();

    let ws_upgrade_success = if is_websocket {
        Some(http_status == Some(101))
    } else {
        None
    };

    Ok(DiagnosticsReport {
        url: url_str.to_string(),
        is_https,
        is_websocket,
        ws_upgrade_success,
        resolved_ips,
        dns_lookup_duration,
        tcp_connect_duration,
        tls_handshake_duration,
        cert_info,
        http_status,
        http_version,
        ttfb,
        total_duration,
        connection_error: None,
    })
}

/// 诊断给定 URL 的 DNS、TCP、TLS 和 HTTP 性能及证书健康度（严格契约：
/// 连接/TLS 阶段失败返回 Err，diagnose 命令的失败退出语义依赖于此）
pub async fn diagnose_url(url_str: &str) -> Result<DiagnosticsReport, String> {
    let start_all = Instant::now();
    let target = resolve_target(url_str)?;

    // 1 & 2. 结合超时进行统一的 DNS + TCP 握手诊断测量（失败 → Err）
    let probe = crate::http::timing::DiagnosticsProber::connect_tcp_staged(
        &target.host,
        target.port,
        DIAGNOSE_STAGE_TIMEOUT,
    )
    .await;
    let Some(stream) = probe.stream else {
        return Err(probe
            .failure
            .unwrap_or_else(|| "connection failed".to_string()));
    };

    probe_after_connect(
        stream,
        probe.socket_addr.unwrap(),
        url_str,
        &target,
        start_all,
        probe.dns_lookup_duration,
        probe.tcp_connect_duration.unwrap_or_default(),
        false,
    )
    .await
}

/// 部分诊断报告组装（连接阶段或 TLS/HTTP 探测阶段失败时的降级产物）：
/// 已测得的阶段数据原样保留，未达成的阶段为空，`connection_error` 携带
/// 首个失败阶段的分类信息。
#[allow(clippy::too_many_arguments)]
fn partial_connection_report(
    url_str: &str,
    is_https: bool,
    is_websocket: bool,
    resolved_ips: Vec<String>,
    dns_lookup_duration: Duration,
    tcp_connect_duration: Duration,
    total_duration: Duration,
    connection_error: String,
) -> DiagnosticsReport {
    DiagnosticsReport {
        url: url_str.to_string(),
        is_https,
        is_websocket,
        ws_upgrade_success: None,
        resolved_ips,
        dns_lookup_duration,
        tcp_connect_duration,
        tls_handshake_duration: None,
        cert_info: None,
        http_status: None,
        http_version: None,
        ttfb: None,
        total_duration,
        connection_error: Some(connection_error),
    }
}

/// 宽容版诊断：DNS/TCP/TLS/HTTP 任一阶段失败都不返回 Err，产出带
/// `connection_error` 阶段分类的部分报告（已解析 IP 与已测耗时保留，未达成
/// 阶段为空），供测试执行器在传输层失败时仍能输出诊断瀑布
/// （DX-Debug D2 连接层拆分；审查扩展：TLS/HTTP 阶段失败同样降级）。
/// 边界：仅 URL 本身非法时返回 Err。
pub async fn diagnose_url_lenient(url_str: &str) -> Result<DiagnosticsReport, String> {
    let start_all = Instant::now();
    let target = resolve_target(url_str)?;

    let probe = crate::http::timing::DiagnosticsProber::connect_tcp_staged(
        &target.host,
        target.port,
        DIAGNOSE_STAGE_TIMEOUT,
    )
    .await;
    let Some(stream) = probe.stream else {
        return Ok(partial_connection_report(
            url_str,
            target.is_https,
            target.is_websocket,
            probe
                .socket_addr
                .map(|a| vec![a.ip().to_string()])
                .unwrap_or_default(),
            probe.dns_lookup_duration,
            probe.tcp_connect_duration.unwrap_or_default(),
            start_all.elapsed(),
            probe
                .failure
                .unwrap_or_else(|| "connection failed".to_string()),
        ));
    };

    probe_after_connect(
        stream,
        probe.socket_addr.unwrap(),
        url_str,
        &target,
        start_all,
        probe.dns_lookup_duration,
        probe.tcp_connect_duration.unwrap_or_default(),
        true,
    )
    .await
}

/// 极致美学格式化输出诊断报告
pub fn print_diagnose_report(report: &DiagnosticsReport) {
    println!(
        "\n{}",
        "=========================================".cyan().bold()
    );
    println!("      {}", "RuPost Network Diagnostics".cyan().bold());
    println!(
        "{}\n",
        "=========================================".cyan().bold()
    );

    println!("{:<14}: {}", "Target URL", report.url);

    let scheme_str = if report.is_websocket {
        if report.is_https {
            "WSS".magenta().bold()
        } else {
            "WS".cyan().bold()
        }
    } else {
        if report.is_https {
            "HTTPS".green().bold()
        } else {
            "HTTP".yellow().bold()
        }
    };
    println!("{:<14}: {}", "Scheme", scheme_str);

    println!(
        "{:<14}: [{}]",
        "Resolved IPs",
        report.resolved_ips.join(", ").blue()
    );

    // 连接阶段失败分类（部分诊断报告特有；完整报告恒为 None 不打印）
    if let Some(ref err) = report.connection_error {
        println!("{:<14}: {}", "Connection", err.red().bold());
    }
    println!();

    println!("{}", "Latency Breakdown (Waterfall)".bold());
    println!("{}", "-----------------------------------------".cyan());

    let dns_ms = report.dns_lookup_duration.as_secs_f64() * 1000.0;
    let tcp_ms = report.tcp_connect_duration.as_secs_f64() * 1000.0;
    let tls_ms = report
        .tls_handshake_duration
        .map(|d| d.as_secs_f64() * 1000.0);
    let ttfb_ms = report.ttfb.map(|d| d.as_secs_f64() * 1000.0);
    let total_ms = report.total_duration.as_secs_f64() * 1000.0;

    fn get_bar(ms: f64, max_ms: f64) -> String {
        let width = 20;
        let filled = if max_ms > 0.0 {
            ((ms / max_ms) * width as f64).round() as usize
        } else {
            0
        };
        let filled = filled.clamp(1, width);
        let mut bar = "█".repeat(filled);
        if filled < width {
            bar.push_str(&"░".repeat(width - filled));
        }
        bar
    }

    let max_ms = total_ms;

    println!(
        "{:<14} : {} {:>6.1} ms",
        "DNS Lookup",
        get_bar(dns_ms, max_ms).blue(),
        dns_ms
    );
    println!(
        "{:<14} : {} {:>6.1} ms",
        "TCP Connect",
        get_bar(tcp_ms, max_ms).green(),
        tcp_ms
    );
    if let Some(tls) = tls_ms {
        println!(
            "{:<14} : {} {:>6.1} ms",
            "TLS Handshake",
            get_bar(tls, max_ms).magenta(),
            tls
        );
    }
    if let Some(ttfb) = ttfb_ms {
        println!(
            "{:<14} : {} {:>6.1} ms",
            "HTTP TTFB",
            get_bar(ttfb, max_ms).yellow(),
            ttfb
        );
    }
    println!("{:<14} : {:>6.1} ms", "Total Latency", total_ms);
    println!();

    if report.is_https {
        println!("{}", "TLS Certificate Info".bold());
        println!("{}", "-----------------------------------------".cyan());
        if let Some(cert) = &report.cert_info {
            println!("{:<14}: {}", "Subject (SAN)", cert.subject.blue());
            println!("{:<14}: {}", "Issuer", cert.issuer);
            println!("{:<14}: {}", "Valid Until", cert.validity_not_after);

            let days = cert.days_remaining;
            let days_str = format!("{} days", days);
            let (status_str, days_colored) = if days <= 0 {
                ("[EXPIRED]".red().bold(), days_str.red().bold())
            } else if days <= 30 {
                ("[WARNING]".yellow().bold(), days_str.yellow().bold())
            } else {
                ("[VALID]".green().bold(), days_str.green().bold())
            };
            println!("{:<14}: {} ({})", "Status", status_str, days_colored);
        } else {
            println!("{}", "Failed to extract certificate information.".red());
        }
        println!();
    }

    if report.is_websocket {
        println!("{}", "WebSocket Handshake Info".bold());
        println!("{}", "-----------------------------------------".cyan());
        match report.ws_upgrade_success {
            Some(true) => {
                println!(
                    "{} {}",
                    "[WS UPGRADE SUCCESS]".green().bold(),
                    "WebSocket Upgrade completed successfully (101 Switching Protocols)".white()
                );
            }
            Some(false) | None => {
                println!(
                    "{} {}",
                    "[WS UPGRADE FAILED]".red().bold(),
                    "WebSocket Upgrade handshake failed!".red()
                );
                println!(
                    "{}",
                    "Hint: Did not return 101 Switching Protocols. Check if your Nginx/Gateway\n   is missing 'Upgrade' and 'Connection' headers configuration!"
                        .yellow()
                );
            }
        }
        println!();
    } else {
        println!("{}", "HTTP Protocol Info".bold());
        println!("{}", "-----------------------------------------".cyan());
        if let Some(status) = report.http_status {
            let status_colored = if status >= 400 {
                status.to_string().red().bold()
            } else {
                status.to_string().green().bold()
            };
            println!("{:<14}: {}", "Status Code", status_colored);
        } else {
            println!("{:<14}: {}", "Status Code", "Failed to get response".red());
        }
        if let Some(version) = &report.http_version {
            println!("{:<14}: {}", "HTTP Version", version);
        }
    }
    println!(
        "{}",
        "=========================================\n".cyan().bold()
    );
}
