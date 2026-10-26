use crate::Result;
use crate::exporter::plugin::CommandExporter;
use crate::exporter::shell::escape_shell_word;
use crate::exporter::types::{ExportOptions, ExportableRequest};

pub struct HttpieExporter;

impl CommandExporter for HttpieExporter {
    fn name(&self) -> &'static str {
        "httpie"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["http"]
    }

    fn description(&self) -> &'static str {
        "Export request as HTTPie CLI command"
    }

    fn export_one(&self, req: &ExportableRequest, opts: &ExportOptions) -> Result<String> {
        let mut parts: Vec<String> = vec!["http".to_string()];

        // 在管道或脚本中运行时防止无 body 请求挂起等待 stdin
        if req.body.is_none() {
            parts.push("--ignore-stdin".to_string());
        }

        if req.is_sse {
            parts.push("--stream".to_string());
        }

        if let Some(secs) = req.timeout_secs {
            parts.push(format!("--timeout={secs}"));
        }

        if let Some(ref body) = req.body {
            parts.push(format!("--raw {}", escape_shell_word(body, opts.compact)));
        }

        // Method 与 URL
        parts.push(req.method.to_uppercase());
        parts.push(escape_shell_word(&req.url, opts.compact));

        // Headers
        for (k, v) in &req.headers {
            // HTTPie 语法：空值 Header 用 'Key;'，非空用 'Key:Value'（'Key:' 表示删除该头）
            let item = if v.is_empty() {
                format!("{k};")
            } else {
                format!("{k}:{v}")
            };
            parts.push(escape_shell_word(&item, opts.compact));
        }

        if opts.compact || parts.len() <= 2 {
            Ok(parts.join(" "))
        } else {
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
    fn test_httpie_get_simple() {
        let req = sample_req("GET", "https://api.example.com/users");
        let opts = ExportOptions {
            compact: true,
            ..Default::default()
        };
        let out = HttpieExporter.export_one(&req, &opts).unwrap();
        assert_eq!(
            out,
            "http --ignore-stdin GET 'https://api.example.com/users'"
        );
    }

    #[test]
    fn test_httpie_post_json() {
        let mut req = sample_req("POST", "https://api.example.com/login");
        req.body = Some("{\"user\": \"bob\"}".to_string());
        req.headers
            .push(("Authorization".into(), "Bearer xyz".into()));
        req.headers.push(("X-Empty".into(), "".into()));

        let out = HttpieExporter
            .export_one(&req, &ExportOptions::default())
            .unwrap();
        // 验证有 body 时不带 --ignore-stdin，使用 --raw
        assert!(!out.contains("--ignore-stdin"));
        assert!(out.contains("--raw '{\"user\": \"bob\"}'"));
        assert!(out.contains("POST"));
        assert!(out.contains("'https://api.example.com/login'"));
        assert!(out.contains("'Authorization:Bearer xyz'"));
        // 空值 Header 转义为分号 ';'
        assert!(out.contains("'X-Empty;'"));
        assert!(out.contains(" \\\n  "));
    }

    #[test]
    fn test_httpie_sse_stream() {
        let mut req = sample_req("GET", "https://api.example.com/events");
        req.is_sse = true;
        req.timeout_secs = Some(30);

        let out = HttpieExporter
            .export_one(&req, &ExportOptions::default())
            .unwrap();
        assert!(out.contains("--stream"));
        assert!(out.contains("--timeout=30"));
    }
}
