use crate::exporter::redact::{redact_header_value, redact_url};
use crate::exporter::types::{ExportableRequest, SkippedItem};
use crate::http::url::resolve_final_url;
use crate::parser::{ParsedFile, ParsedRequest};
use crate::variable::{VariableContext, VariableResolver};
use crate::{Result, RupostError};
use cookie_store::CookieStore;
use std::collections::HashSet;

/// 文件准备产物，包含归一化后的请求列表与被跳过的请求项
pub struct PreparedFileOutput {
    pub requests: Vec<ExportableRequest>,
    pub skipped: Vec<SkippedItem>,
}

/// 请求归一化与准备管线
pub struct RequestPreparer;

impl RequestPreparer {
    /// 将整份 `ParsedFile` 中的请求按执行期规则过滤并归一化为 `ExportableRequest`
    pub fn prepare_file(
        parsed_file: &ParsedFile,
        context: &mut VariableContext,
        cookie_store: Option<&CookieStore>,
        filter_name: Option<&str>,
        redact_secrets: bool,
        captured_symbols: &mut HashSet<String>,
    ) -> Result<PreparedFileOutput> {
        let mut requests = Vec::new();
        let mut skipped = Vec::new();

        let source_label = parsed_file
            .source_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        let has_explicit_tests = parsed_file.requests.iter().any(|r| r.metadata.is_test);

        for req in &parsed_file.requests {
            // 收集当前请求声明的 @capture 变量名
            let req_captures: Vec<String> = req
                .metadata
                .captures
                .iter()
                .map(|c| c.name.clone())
                .collect();

            // 1. 若指定了 --name，按用例名精确匹配过滤
            if let Some(target_name) = filter_name {
                if req.name() != Some(target_name) {
                    captured_symbols.extend(req_captures);
                    continue;
                }
            } else {
                // 2. 默认跳过规则：@skip、非 @test（当文件存在显式 @test 时）、服务端 Mock 路由定义块
                if req.should_skip() {
                    skipped.push(SkippedItem {
                        name: req.name().map(str::to_string),
                        line_number: req.line_number,
                        reason: "@skip".to_string(),
                    });
                    captured_symbols.extend(req_captures);
                    continue;
                }
                if has_explicit_tests && !req.metadata.is_test {
                    skipped.push(SkippedItem {
                        name: req.name().map(str::to_string),
                        line_number: req.line_number,
                        reason: "non-@test".to_string(),
                    });
                    captured_symbols.extend(req_captures);
                    continue;
                }
                if !req.metadata.mock_variants.is_empty() && !req.metadata.is_test {
                    skipped.push(SkippedItem {
                        name: req.name().map(str::to_string),
                        line_number: req.line_number,
                        reason: "mock route definition".to_string(),
                    });
                    captured_symbols.extend(req_captures);
                    continue;
                }
            }

            // 3. 拦截 WebSocket 请求（避免将交互剧本输出为普通 HTTP Body）
            let is_ws = req.metadata.websocket
                || req.url.starts_with("ws://")
                || req.url.starts_with("wss://");
            if is_ws {
                skipped.push(SkippedItem {
                    name: req.name().map(str::to_string),
                    line_number: req.line_number,
                    reason: "WebSocket protocol is not supported by HTTP command exporters"
                        .to_string(),
                });
                captured_symbols.extend(req_captures);
                continue;
            }

            let ir = Self::prepare_one(
                req.clone(),
                context,
                cookie_store,
                source_label.as_deref(),
                redact_secrets,
                captured_symbols,
            )?;

            captured_symbols.extend(req_captures);
            requests.push(ir);
        }

        Ok(PreparedFileOutput { requests, skipped })
    }

    /// 归一化单个 `ParsedRequest`
    pub fn prepare_one(
        mut parsed: ParsedRequest,
        context: &mut VariableContext,
        cookie_store: Option<&CookieStore>,
        source_path: Option<&str>,
        redact_secrets: bool,
        captured_symbols: &HashSet<String>,
    ) -> Result<ExportableRequest> {
        // 1. 变量与环境替换（含全局 user_agent 自动注入）
        VariableResolver::resolve_parsed_request(&mut parsed, context);

        // 2. 检查未定义占位符，区分“前置 @capture 运行时变量”与“未知变量”
        let mut warnings = Vec::new();
        for raw_warn in VariableResolver::unresolved_placeholders(&parsed) {
            let hit_capture = captured_symbols
                .iter()
                .find(|sym| raw_warn.contains(&format!("{{{{{sym}}}}}")));
            if let Some(sym) = hit_capture {
                warnings.push(format!(
                    "变量 {{{{{sym}}}}} 来源于前置请求的运行时 @capture，静态导出保留占位符（可通过 --var {sym}=<VAL> 指定）"
                ));
            } else {
                warnings.push(raw_warn);
            }
        }

        if parsed.url.contains("{{base_url}}") || parsed.url.contains("{{baseUrl}}") {
            return Err(RupostError::BaseUrlNotConfigured);
        }

        // 3. 解析并拼接最终 URL
        let default_scheme = context
            .get("__default_scheme")
            .unwrap_or_else(|| "http".to_string());

        let final_url = resolve_final_url(
            &parsed.url,
            context,
            &default_scheme,
            source_path,
            parsed.base_path.as_deref(),
        )?;

        let name = parsed.name().map(str::to_string);
        let line_number = parsed.line_number;
        let method = parsed.method_or_default().to_uppercase();
        let body = parsed.body;

        // 4. 过滤底层自计算协议头，并推断隐式 JSON Content-Type
        let mut headers: Vec<(String, String)> = parsed
            .headers
            .into_iter()
            .filter(|(k, _)| {
                !matches!(
                    k.to_ascii_lowercase().as_str(),
                    "content-length" | "connection" | "accept-encoding"
                )
            })
            .collect();

        let has_content_type = headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
        if !has_content_type
            && let Some(ref b) = body
            && is_valid_json_object_or_array(b)
        {
            headers.push(("Content-Type".to_string(), "application/json".to_string()));
        }

        // 5. 若提供了 CookieStore 且请求未手写 Cookie 头，按目标 URL 匹配并注入 Cookie Header
        let has_cookie_header = headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("cookie"));
        if !has_cookie_header
            && let Some(store) = cookie_store
            && let Ok(parsed_url) = url::Url::parse(&final_url)
        {
            let pairs: Vec<String> = store
                .matches(&parsed_url)
                .into_iter()
                .map(|c| format!("{}={}", c.name(), c.value()))
                .collect();
            if !pairs.is_empty() {
                headers.push(("Cookie".to_string(), pairs.join("; ")));
            }
        }

        // 6. 识别 SSE 与超时豁免规则（对齐 request_builder）
        let accepts_sse = headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("accept") && v.contains("text/event-stream"));
        let is_sse = parsed.metadata.sse || accepts_sse;
        let timeout_secs = if is_sse {
            parsed.metadata.sse_timeout.map(|d| d.as_secs().max(1))
        } else {
            parsed.metadata.timeout.map(|d| d.as_secs().max(1))
        };

        // 7. 可选脱敏处理
        let url_out = if redact_secrets {
            redact_url(&final_url)
        } else {
            final_url
        };
        if redact_secrets {
            for (k, v) in &mut headers {
                *v = redact_header_value(k, v);
            }
        }

        let source_location = source_path.map(|p| format!("{p}:{line_number}"));

        Ok(ExportableRequest {
            name,
            source_location,
            method,
            url: url_out,
            headers,
            body,
            timeout_secs,
            is_sse,
            warnings,
        })
    }
}

fn is_valid_json_object_or_array(body: &str) -> bool {
    let trimmed = body.trim();
    let looks_json = (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'));
    looks_json && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variable::capture::VariableCapture;
    use std::time::Duration;

    #[test]
    fn test_prepare_one_auto_json_content_type() {
        let mut parsed = ParsedRequest::new(10);
        parsed.method = Some("POST".to_string());
        parsed.url = "https://api.example.com/users".to_string();
        parsed.body = Some("{\"name\": \"Alice\", \"age\": 30}".to_string());

        let mut context = VariableContext::new();
        let ir = RequestPreparer::prepare_one(
            parsed,
            &mut context,
            None,
            Some("test.http"),
            false,
            &HashSet::new(),
        )
        .unwrap();

        assert_eq!(ir.method, "POST");
        assert_eq!(ir.url, "https://api.example.com/users");
        assert_eq!(ir.source_location.as_deref(), Some("test.http:10"));
        assert!(
            ir.headers
                .iter()
                .any(|(k, v)| k == "Content-Type" && v == "application/json")
        );
    }

    #[test]
    fn test_prepare_one_preserves_explicit_content_type() {
        let mut parsed = ParsedRequest::new(5);
        parsed.method = Some("POST".to_string());
        parsed.url = "https://api.example.com/custom".to_string();
        parsed.headers.push((
            "Content-Type".to_string(),
            "application/vnd.custom+json".to_string(),
        ));
        parsed.body = Some("{\"key\": 1}".to_string());

        let mut context = VariableContext::new();
        let ir =
            RequestPreparer::prepare_one(parsed, &mut context, None, None, false, &HashSet::new())
                .unwrap();

        let ct = ir
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .unwrap();
        assert_eq!(ct.1, "application/vnd.custom+json");
    }

    #[test]
    fn test_prepare_one_redaction() {
        let mut parsed = ParsedRequest::new(1);
        parsed.method = Some("GET".to_string());
        parsed.url = "https://api.example.com/data?token=secret123&page=1".to_string();
        parsed.headers.push((
            "Authorization".to_string(),
            "Bearer eyJhbGci...".to_string(),
        ));
        parsed
            .headers
            .push(("X-Api-Key".to_string(), "key999".to_string()));

        let mut context = VariableContext::new();
        let ir = RequestPreparer::prepare_one(
            parsed,
            &mut context,
            None,
            None,
            true, // redact_secrets = true
            &HashSet::new(),
        )
        .unwrap();

        assert!(!ir.url.contains("secret123"));
        assert!(ir.url.contains("token=%5BREDACTED%5D") || ir.url.contains("token=[REDACTED]"));
        assert_eq!(
            ir.headers
                .iter()
                .find(|(k, _)| k == "Authorization")
                .unwrap()
                .1,
            "Bearer [REDACTED]"
        );
        assert_eq!(
            ir.headers.iter().find(|(k, _)| k == "X-Api-Key").unwrap().1,
            "[REDACTED]"
        );
    }

    #[test]
    fn test_prepare_file_skip_and_capture_tracking() {
        let mut file = ParsedFile::new();

        // 请求 1: 登录，声明 @capture token
        let mut req1 = ParsedRequest::new(1);
        req1.metadata.name = Some("Login".to_string());
        req1.method = Some("POST".to_string());
        req1.url = "https://api.example.com/login".to_string();
        req1.metadata
            .captures
            .push(VariableCapture::from_body("token", "data.token"));
        file.add_request(req1);

        // 请求 2: 标记 @skip
        let mut req2 = ParsedRequest::new(10);
        req2.metadata.skip = true;
        req2.url = "https://api.example.com/skipped".to_string();
        file.add_request(req2);

        // 请求 3: 引用 {{token}}
        let mut req3 = ParsedRequest::new(20);
        req3.method = Some("GET".to_string());
        req3.url = "https://api.example.com/profile".to_string();
        req3.headers
            .push(("Authorization".to_string(), "Bearer {{token}}".to_string()));
        file.add_request(req3);

        let mut context = VariableContext::new();
        let mut captured_symbols = HashSet::new();

        let output = RequestPreparer::prepare_file(
            &file,
            &mut context,
            None,
            None,
            false,
            &mut captured_symbols,
        )
        .unwrap();

        assert_eq!(output.requests.len(), 2); // req1 和 req3
        assert_eq!(output.skipped.len(), 1); // req2
        assert_eq!(output.skipped[0].reason, "@skip");

        // 验证 req3 中的 {{token}} 被精准诊断为来源于前置 @capture
        let profile_req = &output.requests[1];
        assert_eq!(profile_req.url, "https://api.example.com/profile");
        assert!(
            profile_req
                .warnings
                .iter()
                .any(|w| w.contains("来源于前置请求的运行时 @capture"))
        );
    }

    #[test]
    fn test_prepare_file_timeout_and_sse_exemption() {
        let mut parsed = ParsedRequest::new(1);
        parsed.method = Some("GET".to_string());
        parsed.url = "https://api.example.com/stream".to_string();
        parsed.metadata.sse = true;
        parsed.metadata.timeout = Some(Duration::from_secs(30)); // 普通超时应被豁免
        parsed.metadata.sse_timeout = Some(Duration::from_secs(120)); // 应生效专属超时

        let mut context = VariableContext::new();
        let ir =
            RequestPreparer::prepare_one(parsed, &mut context, None, None, false, &HashSet::new())
                .unwrap();

        assert!(ir.is_sse);
        assert_eq!(ir.timeout_secs, Some(120));
    }
}
