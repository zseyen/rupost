use crate::Result;
use crate::exporter::plugin::CommandExporter;
use crate::exporter::shell::escape_shell_word;
use crate::exporter::types::{ExportOptions, ExportableRequest};

pub struct CurlExporter;

impl CommandExporter for CurlExporter {
    fn name(&self) -> &'static str {
        "curl"
    }

    fn description(&self) -> &'static str {
        "Export request as standard cURL command"
    }

    fn export_one(&self, req: &ExportableRequest, opts: &ExportOptions) -> Result<String> {
        let mut parts: Vec<String> = vec!["curl".to_string()];
        let method = req.method.to_uppercase();
        let has_body = req.body.is_some();

        if method == "HEAD" {
            // 严禁使用 -X HEAD，避免 curl 等待响应体导致挂死
            parts.push("-I".to_string());
        } else if has_body || method != "GET" {
            // 带 Body 时即使是 GET 也必须显式 -X GET，防止被 --data-raw 静默改写为 POST
            parts.push(format!("-X {method}"));
        }

        if req.is_sse {
            parts.push("-N".to_string());
        }

        if let Some(secs) = req.timeout_secs {
            parts.push(format!("--max-time {secs}"));
        }

        for (k, v) in &req.headers {
            let header = format!("{k}: {v}");
            parts.push(format!("-H {}", escape_shell_word(&header, opts.compact)));
        }

        if let Some(ref body) = req.body {
            parts.push(format!(
                "--data-raw {}",
                escape_shell_word(body, opts.compact)
            ));
        }

        parts.push(escape_shell_word(&req.url, opts.compact));

        if opts.compact || parts.len() <= 2 {
            Ok(parts.join(" "))
        } else {
            // 反斜杠后紧跟换行符，绝不可包含尾随空格
            Ok(parts.join(" \\\n  "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_req(method: &str, url: &str) -> ExportableRequest {
        ExportableRequest {
            name: Some("Test".to_string()),
            source_location: None,
            method: method.to_string(),
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
            timeout_secs: None,
            is_sse: false,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn test_curl_get_simple() {
        let req = sample_req("GET", "https://api.example.com/users");
        let out = CurlExporter
            .export_one(&req, &ExportOptions::default())
            .unwrap();
        assert_eq!(out, "curl 'https://api.example.com/users'");
    }

    #[test]
    fn test_curl_head_uses_capital_i() {
        let req = sample_req("HEAD", "https://api.example.com/users");
        let out = CurlExporter
            .export_one(&req, &ExportOptions::default())
            .unwrap();
        assert!(out.contains("-I"));
        assert!(!out.contains("-X HEAD"));
    }

    #[test]
    fn test_curl_post_json_with_body() {
        let mut req = sample_req("POST", "https://api.example.com/login");
        req.headers
            .push(("Content-Type".into(), "application/json".into()));
        req.body = Some("{\"user\": \"alice\"}".to_string());

        let out = CurlExporter
            .export_one(&req, &ExportOptions::default())
            .unwrap();
        assert!(out.contains("-X POST"));
        assert!(out.contains("-H 'Content-Type: application/json'"));
        assert!(out.contains("--data-raw '{\"user\": \"alice\"}'"));
        assert!(out.contains(" \\\n  "));
    }

    #[test]
    fn test_curl_get_with_body_preserves_method() {
        let mut req = sample_req("GET", "https://api.example.com/search");
        req.body = Some("{\"query\": \"rust\"}".to_string());

        let out = CurlExporter
            .export_one(&req, &ExportOptions::default())
            .unwrap();
        // 关键：带 body 时即使是 GET 也必须有 -X GET，防止 curl 改写为 POST
        assert!(out.contains("-X GET"));
        assert!(out.contains("--data-raw '{\"query\": \"rust\"}'"));
    }

    #[test]
    fn test_curl_sse_and_timeout() {
        let mut req = sample_req("GET", "https://api.example.com/stream");
        req.is_sse = true;
        req.timeout_secs = Some(60);

        let out = CurlExporter
            .export_one(&req, &ExportOptions::default())
            .unwrap();
        assert!(out.contains("-N"));
        assert!(out.contains("--max-time 60"));
    }

    #[test]
    fn test_curl_compact_mode() {
        let mut req = sample_req("POST", "https://api.example.com/data");
        req.body = Some("line1\nline2".to_string());

        let opts = ExportOptions {
            compact: true,
            ..Default::default()
        };
        let out = CurlExporter.export_one(&req, &opts).unwrap();
        assert!(!out.contains(" \\\n  "));
        assert!(out.contains("$'line1\\nline2'"));
    }
}
