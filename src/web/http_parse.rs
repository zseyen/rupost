use crate::parser::http_file::HttpFileParser;
use crate::parser::types::ParsedRequest;
use crate::web::types::{HttpCardDto, HttpHeaderDto, ParseHttpResponse};
use std::collections::BTreeMap;

/// 将领域模型 `ParsedRequest` 转换为前端统一的 `HttpCardDto`（`.http` 与 `.md` 共用 SSOT）
pub fn parsed_request_to_card_dto(req: &ParsedRequest, raw_block: String) -> HttpCardDto {
    let method = if req.metadata.websocket && req.method.is_none() {
        "WS".to_string()
    } else {
        req.method_or_default().to_uppercase()
    };

    let title = req
        .metadata
        .name
        .clone()
        .unwrap_or_else(|| format!("{} {}", method, req.url));

    let headers = req
        .headers
        .iter()
        .map(|(k, v)| HttpHeaderDto {
            key: k.clone(),
            value: v.clone(),
        })
        .collect();

    let mut directives = Vec::new();
    if let Some(name) = &req.metadata.name {
        directives.push(format!("@name {name}"));
    }
    if req.metadata.skip {
        directives.push("@skip".to_string());
    }
    if req.metadata.websocket {
        directives.push("@websocket".to_string());
    }
    if req.metadata.sse {
        directives.push("@sse".to_string());
    }
    if req.metadata.diagnose {
        directives.push("@diagnose".to_string());
    }
    if req.metadata.is_teardown {
        directives.push("@teardown".to_string());
    }
    for cap in &req.metadata.captures {
        directives.push(format!("@capture {} from {}", cap.name, cap.source));
    }
    for assert_expr in &req.metadata.assertions {
        directives.push(format!("@assert {assert_expr}"));
    }
    if !req.metadata.mock_variants.is_empty() {
        directives.push(format!(
            "@mock ({} variants)",
            req.metadata.mock_variants.len()
        ));
    }

    HttpCardDto {
        title,
        method,
        url: req.url.clone(),
        headers,
        body: req.body.clone(),
        directives,
        has_mock: !req.metadata.mock_variants.is_empty(),
        mock_variant_count: req.metadata.mock_variants.len(),
        is_websocket: req.metadata.websocket,
        raw_block,
    }
}

/// 根据每个 `ParsedRequest.line_number` 从原文精确切片出所属 `###` 区块原文
fn slice_raw_blocks(content: &str, requests: &[ParsedRequest]) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    if requests.is_empty() || lines.is_empty() {
        return vec![String::new(); requests.len()];
    }

    let mut start_indices = Vec::with_capacity(requests.len());
    for (i, req) in requests.iter().enumerate() {
        let anchor = req
            .line_number
            .saturating_sub(1)
            .min(lines.len().saturating_sub(1));
        let floor = if i == 0 {
            0
        } else {
            requests[i - 1].line_number.min(anchor)
        };
        let mut block_start = anchor;
        let mut cur = anchor;
        while cur > floor {
            cur -= 1;
            if lines[cur].trim_start().starts_with("###") {
                block_start = cur;
                break;
            }
        }
        start_indices.push(block_start);
    }

    let mut out = Vec::with_capacity(requests.len());
    for i in 0..start_indices.len() {
        let start = start_indices[i];
        let end = if i + 1 < start_indices.len() {
            start_indices[i + 1]
        } else {
            lines.len()
        };
        out.push(lines[start..end].join("\n").trim().to_string());
    }
    out
}

/// 提取 `.http` 文件中各 `###` 块请求行之前的 `@ident = value` 变量声明（复用 HttpFileParser SSOT）
fn extract_file_variables(content: &str) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    let mut seen_request_line = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("###") {
            seen_request_line = false;
            continue;
        }
        if seen_request_line
            || trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("//")
        {
            continue;
        }
        if let Some((k, v)) = HttpFileParser::parse_var_assignment(trimmed) {
            vars.insert(k, v);
            continue;
        }
        if !trimmed.starts_with('@') {
            seen_request_line = true;
        }
    }
    vars
}

/// 使用权威 `HttpFileParser` 解析 `.http` 内容为结构化卡片列表
pub fn parse_http_document_to_cards(content: &str) -> Result<ParseHttpResponse, String> {
    let parsed = HttpFileParser::parse_content(content).map_err(|e| e.to_string())?;
    let raw_blocks = slice_raw_blocks(content, &parsed.requests);

    let cards = parsed
        .requests
        .iter()
        .zip(raw_blocks)
        .map(|(req, raw)| parsed_request_to_card_dto(req, raw))
        .collect();

    let variables = extract_file_variables(content);
    Ok(ParseHttpResponse { variables, cards })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_http_document_ignores_preamble_comment_and_trailing_assert_not_in_body() {
        let src = "\
# 获取用户资料 (对本地 mock 发起，预期 401 失败用于分诊演示)
### get-profile
# @name get-profile
GET {{base_url}}/v1/users/1
Accept: application/json

# @assert status == 200
";
        let res = parse_http_document_to_cards(src).unwrap();
        assert_eq!(res.cards.len(), 1, "前置纯注释不得被渲染为空卡片");
        let card = &res.cards[0];
        assert_eq!(card.title, "get-profile");
        assert_eq!(card.method, "GET");
        assert_eq!(card.url, "{{base_url}}/v1/users/1");
        assert!(
            card.body.is_none(),
            "空行后的 # @assert 不得被误认为 Request Body"
        );
        assert!(
            card.directives.iter().any(|d| d == "@assert status == 200"),
            "directives 应包含 @assert status == 200: {:?}",
            card.directives
        );
        assert!(card.raw_block.contains("GET {{base_url}}/v1/users/1"));
    }

    #[test]
    fn test_parse_http_document_extracts_file_variables_and_multiple_blocks() {
        let src = "\
@tenant = east_01
@base_url = https://httpbingo.org

### Get
GET {{base_url}}/get?t={{tenant}}

### Post
POST {{base_url}}/post
Content-Type: application/json

{\"ok\": true}
";
        let res = parse_http_document_to_cards(src).unwrap();
        assert_eq!(
            res.variables.get("tenant").map(String::as_str),
            Some("east_01")
        );
        assert_eq!(res.cards.len(), 2);
        assert_eq!(res.cards[1].method, "POST");
        assert_eq!(res.cards[1].body.as_deref(), Some("{\"ok\": true}"));
    }

    #[test]
    fn test_extract_file_variables_contract_table() {
        type VarCase<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a [&'a str]);
        let cases: &[VarCase<'_>] = &[
            (
                "### Config\n@token = sec_123\nGET https://example.com\n",
                &[("token", "sec_123")],
                &[],
            ),
            (
                "POST https://example.com\n\n@not_a_var = 1\n",
                &[],
                &["not_a_var"],
            ),
            (
                "@bad-key = 1\n@good_key_2 = ok\nGET https://example.com\n",
                &[("good_key_2", "ok")],
                &["bad-key"],
            ),
            (
                "@env_host = a\n\n### Second\n@env_host = b\nGET https://example.com\n",
                &[("env_host", "b")],
                &[],
            ),
        ];

        for (src, expected_present, expected_absent) in cases {
            let vars = extract_file_variables(src);
            for (k, v) in *expected_present {
                assert_eq!(vars.get(*k).map(String::as_str), Some(*v), "src: {src}");
            }
            for k in *expected_absent {
                assert!(!vars.contains_key(*k), "unexpected key {k} in src: {src}");
            }
        }
    }
}
