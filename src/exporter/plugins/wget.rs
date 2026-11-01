use crate::Result;
use crate::exporter::plugin::CommandExporter;
use crate::exporter::shell::escape_shell_word;
use crate::exporter::types::{ExportOptions, ExportableRequest};

pub struct WgetExporter;

impl CommandExporter for WgetExporter {
    fn name(&self) -> &'static str {
        "wget"
    }

    fn description(&self) -> &'static str {
        "Export request as GNU Wget command"
    }

    fn export_one(&self, req: &ExportableRequest, opts: &ExportOptions) -> Result<String> {
        let mut parts: Vec<String> = vec!["wget".to_string()];
        let method = req.method.to_uppercase();

        if method == "HEAD" {
            parts.push("-q".to_string());
            parts.push("--spider".to_string());
            parts.push("--server-response".to_string());
        } else {
            parts.push("-qO-".to_string());
            parts.push("--content-on-error".to_string());
            parts.push(format!("--method={method}"));
        }

        if let Some(secs) = req.timeout_secs {
            parts.push(format!("--timeout={secs}"));
        }

        for (k, v) in &req.headers {
            let header = format!("{k}: {v}");
            parts.push(format!(
                "--header={}",
                escape_shell_word(&header, opts.compact)
            ));
        }

        if let Some(ref body) = req.body
            && method != "HEAD"
        {
            parts.push(format!(
                "--body-data={}",
                escape_shell_word(body, opts.compact)
            ));
        }

        parts.push(escape_shell_word(&req.url, opts.compact));

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
    fn test_wget_get_simple() {
        let req = sample_req("GET", "https://api.example.com/users");
        let opts = ExportOptions {
            compact: true,
            ..Default::default()
        };
        let out = WgetExporter.export_one(&req, &opts).unwrap();
        assert_eq!(
            out,
            "wget -qO- --content-on-error --method=GET 'https://api.example.com/users'"
        );
    }

    #[test]
    fn test_wget_head_uses_spider() {
        let req = sample_req("HEAD", "https://api.example.com/status");
        let out = WgetExporter
            .export_one(&req, &ExportOptions::default())
            .unwrap();
        assert!(out.contains("--spider"));
        assert!(out.contains("--server-response"));
        assert!(!out.contains("--method=HEAD"));
    }

    #[test]
    fn test_wget_post_with_body_and_headers() {
        let mut req = sample_req("POST", "https://api.example.com/data");
        req.headers
            .push(("Content-Type".into(), "application/json".into()));
        req.body = Some("{\"k\": \"v\"}".to_string());
        req.timeout_secs = Some(15);

        let out = WgetExporter
            .export_one(&req, &ExportOptions::default())
            .unwrap();
        assert!(out.contains("--content-on-error"));
        assert!(out.contains("--method=POST"));
        assert!(out.contains("--timeout=15"));
        assert!(out.contains("--header='Content-Type: application/json'"));
        assert!(out.contains("--body-data='{\"k\": \"v\"}'"));
    }
}
