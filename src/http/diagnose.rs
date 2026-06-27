use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::{TcpStream, lookup_host};
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

/// 诊断给定 URL 的 DNS、TCP、TLS 和 HTTP 性能及证书健康度
#[allow(unused_assignments)]
pub async fn diagnose_url(url_str: &str) -> Result<DiagnosticsReport, String> {
    let start_all = Instant::now();
    let url = Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;
    let host = url
        .host_str()
        .ok_or_else(|| "Missing host in URL".to_string())?;

    // 协议识别
    let orig_scheme = url.scheme().to_lowercase();
    let is_websocket = orig_scheme == "ws" || orig_scheme == "wss";
    let is_https = orig_scheme == "https" || orig_scheme == "wss";

    // 解析端口或默认端口
    let port = url
        .port_or_known_default()
        .unwrap_or(if is_https { 443 } else { 80 });

    // 1. DNS 诊断
    let dns_start = Instant::now();
    let addrs_iter = lookup_host(format!("{}:{}", host, port))
        .await
        .map_err(|e| format!("DNS lookup failed: {}", e))?;
    let dns_lookup_duration = dns_start.elapsed();

    let addrs_vec: Vec<std::net::SocketAddr> = addrs_iter.collect();
    let resolved_ips: Vec<String> = addrs_vec.iter().map(|addr| addr.ip().to_string()).collect();
    let target_addr = resolved_ips
        .first()
        .ok_or_else(|| "No IP addresses resolved".to_string())?
        .clone();

    // 2. TCP 连接诊断
    let tcp_start = Instant::now();
    let tcp_stream = TcpStream::connect(format!("{}:{}", target_addr, port))
        .await
        .map_err(|e| format!("TCP connection failed: {}", e))?;
    let tcp_connect_duration = tcp_start.elapsed();

    let mut tls_handshake_duration = None;
    let mut cert_info = None;
    let mut http_status = None;
    let mut http_version = None;
    let mut ttfb = None;

    // 根据是否是 WebSocket，自适应组装 Upgrade 握手或常规 HTTP 请求
    let request_payload = if is_websocket {
        format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n",
            url.path(),
            host
        )
    } else {
        format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            url.path(),
            host
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
        let mut tls_stream = connector
            .connect(server_name, tcp_stream)
            .await
            .map_err(|e| format!("TLS handshake failed: {}", e))?;
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
        tls_stream
            .write_all(request_payload.as_bytes())
            .await
            .map_err(|e| format!("Failed to send request over TLS: {}", e))?;
        tls_stream.flush().await.ok();

        let mut buffer = [0u8; 1024];
        let n = tls_stream
            .read(&mut buffer)
            .await
            .map_err(|e| format!("Failed to read response over TLS: {}", e))?;
        ttfb = Some(ttfb_start.elapsed());

        if n > 0
            && let Some((status, version)) = parse_http_response_header(&buffer[..n])
        {
            http_status = Some(status);
            http_version = Some(version);
        }
    } else {
        // 4. HTTP TTFB (HTTP)
        let ttfb_start = Instant::now();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tcp_stream;
        stream
            .write_all(request_payload.as_bytes())
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;
        stream.flush().await.ok();

        let mut buffer = [0u8; 1024];
        let n = stream
            .read(&mut buffer)
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;
        ttfb = Some(ttfb_start.elapsed());

        if n > 0
            && let Some((status, version)) = parse_http_response_header(&buffer[..n])
        {
            http_status = Some(status);
            http_version = Some(version);
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
    })
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
        if report.is_https { "WSS".magenta().bold() } else { "WS".cyan().bold() }
    } else {
        if report.is_https { "HTTPS".green().bold() } else { "HTTP".yellow().bold() }
    };
    println!("{:<14}: {}", "Scheme", scheme_str);
    
    println!(
        "{:<14}: [{}]",
        "Resolved IPs",
        report.resolved_ips.join(", ").blue()
    );
    println!();

    println!("{}", "⏱️  Latency Breakdown (Waterfall)".bold());
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
        println!("{}", "🛡️  TLS Certificate Info".bold());
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
        println!("{}", "🔌  WebSocket Handshake Info".bold());
        println!("{}", "-----------------------------------------".cyan());
        match report.ws_upgrade_success {
            Some(true) => {
                println!(
                    "{} {}",
                    "🟢 [WS UPGRADE SUCCESS]".green().bold(),
                    "WebSocket Upgrade completed successfully (101 Switching Protocols)".white()
                );
            }
            Some(false) | None => {
                println!(
                    "{} {}",
                    "❌ [WS UPGRADE FAILED]".red().bold(),
                    "WebSocket Upgrade handshake failed!".red()
                );
                println!(
                    "{}",
                    "💡 Hint: Did not return 101 Switching Protocols. Check if your Nginx/Gateway\n   is missing 'Upgrade' and 'Connection' headers configuration!"
                        .yellow()
                );
            }
        }
        println!();
    } else {
        println!("{}", "🌐  HTTP Protocol Info".bold());
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
