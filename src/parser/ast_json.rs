//! AstDocument JSON 投影（WP-AST，插件生态使能）。
//!
//! 把 `ParsedFile` 投影为**稳定的外部契约**（`schema_version "1.0"`，只增不改），
//! 供命令插件经 `rupost ast <FILE>` / `$RUPOST_BIN ast <file>` 消费——解析语义与
//! 核心零分叉（设计：评估报告 §6/§7/§8）。
//!
//! 契约纪律：**不做解析器类型直接 derive**（内部命名不得泄漏为公共契约）——
//! 本模块是内部类型与外部 JSON 之间的隔离层；内部 `ParsedFile` 重构不得破坏字段。

use crate::parser::ParsedFile;
use serde::Serialize;

/// AstDocument 契约版本（只增不改；主版本升级 = 破坏性变更）
pub const AST_SCHEMA_VERSION: &str = "1.0";

/// 稳定 AST 文档（顶层 4 键精确；1.x 内只增）
#[derive(Debug, Serialize)]
pub struct AstDocument {
    pub schema_version: String,
    /// 解析入口路径（CLI 参数原样的 posix 归一化）
    pub source_path: String,
    /// 文件级 `@depends-on` 原始声明
    pub dependencies: Vec<String>,
    pub requests: Vec<AstRequest>,
}

/// 稳定 AST 请求（方法行 = 原文，`{{var}}` 不插值——插值是执行期职责）
#[derive(Debug, Serialize)]
pub struct AstRequest {
    /// 文件内 1-based 序号
    pub number: usize,
    /// 未指定时按核心语义缺省 "GET"
    pub method: String,
    pub url: String,
    /// 保序键值对（序列化为 `[[k, v], ...]`）
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub name: Option<String>,
    pub skip: bool,
    /// `@assert` 原文列表
    pub assertions: Vec<String>,
    /// `@capture` 原始声明文本（`@capture <name> from <source>`）
    pub captures: Vec<String>,
    /// 方法行 1-based 行号（WP-P2a；缺席为 null）
    pub source_line: Option<usize>,
}

/// 投影：内部解析结果 → 稳定契约（纯函数，无 IO）
pub fn to_ast_document(parsed: &ParsedFile, source_path: &str) -> AstDocument {
    AstDocument {
        schema_version: AST_SCHEMA_VERSION.to_string(),
        source_path: crate::utils::path_guard::normalize_posix_path(source_path),
        dependencies: parsed.dependencies.clone(),
        requests: parsed
            .requests
            .iter()
            .enumerate()
            .map(|(idx, req)| AstRequest {
                number: idx + 1,
                method: req.method_or_default().to_string(),
                url: req.url.clone(),
                headers: req.headers.clone(),
                body: req.body.clone(),
                name: req.metadata.name.clone(),
                skip: req.metadata.skip,
                assertions: req.metadata.assertions.clone(),
                captures: req
                    .metadata
                    .captures
                    .iter()
                    .map(|c| format!("@capture {} from {}", c.name, c.source))
                    .collect(),
                source_line: req.metadata.source_line,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 顶层与请求级键集精确契约（只增不改；新增字段必须显式登记）
    #[test]
    fn test_ast_schema_exact_keys() {
        let content = "### ok\n# @assert status == 200\nGET {{base_url}}/ok\n\n# @capture t from body.token\n";
        let parsed = crate::parser::parse_document_by_path("s.http", content).expect("parse");
        let doc = to_ast_document(&parsed, "s.http");

        let v = serde_json::to_value(&doc).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 4, "top-level keys: {:?}", obj.keys());
        for k in ["schema_version", "source_path", "dependencies", "requests"] {
            assert!(obj.contains_key(k), "missing: {k}");
        }
        assert_eq!(v["schema_version"], "1.0");

        let req = &v["requests"][0];
        let req_obj = req.as_object().unwrap();
        let mut keys: Vec<&str> = req_obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "assertions",
                "body",
                "captures",
                "headers",
                "method",
                "name",
                "number",
                "skip",
                "source_line",
                "url"
            ],
            "request keys: {keys:?}"
        );
        // `{{var}}` 原文保留（插值是执行期职责）
        assert_eq!(req["url"], "{{base_url}}/ok");
        // capture 原始声明文本
        assert_eq!(req["captures"][0], "@capture t from body.token");
    }

    /// headers 保序契约（双头文档）
    #[test]
    fn test_ast_headers_order_preserved() {
        let content = "### h\nGET http://x/\nAuthorization: Bearer t\nX-First: 1\nX-Second: 2\n";
        let parsed = crate::parser::parse_document_by_path("h.http", content).expect("parse");
        let doc = to_ast_document(&parsed, "h.http");
        let v = serde_json::to_value(&doc).unwrap();
        let headers = v["requests"][0]["headers"].as_array().unwrap();
        assert_eq!(headers.len(), 3);
        assert_eq!(headers[0][0], "Authorization");
        assert_eq!(headers[1][0], "X-First");
        assert_eq!(headers[2][0], "X-Second");
    }
}
