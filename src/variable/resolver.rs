use crate::variable::types::VariableContext;
use regex::{Captures, Regex};
use std::sync::OnceLock;

/// 变量替换器
pub struct VariableResolver;

impl VariableResolver {
    /// 替换文本中的所有 {{variable}} 占位符，支持动态内置变量
    ///
    /// 两段式求值：
    /// 1. 函数宏通道（`{{$hmac_sha256(key, msg)}}` 等）：先于普通变量求值，
    ///    宏参数在求值时再做一次嵌套 `{{...}}` 插值（内外层求值序由守护测试锁定）；
    /// 2. 简单变量通道（`{{$uuid}}` / `{{user_id}}`）。
    ///
    /// 求值失败的函数宏保留字面量，交由 `unresolved_placeholders` 显式告警（反静默）。
    pub fn substitute(text: &str, context: &VariableContext) -> String {
        static FN_MACRO_REGEX: OnceLock<Regex> = OnceLock::new();
        static VAR_REGEX: OnceLock<Regex> = OnceLock::new();
        let fn_re =
            FN_MACRO_REGEX.get_or_init(|| Regex::new(r"\{\{\$([a-z_0-9]+)\((.*?)\)\}\}").unwrap());

        // 通道 1：函数宏（带参数）
        let with_fns = fn_re.replace_all(text, |caps: &Captures| {
            let name: &str = &caps[1];
            let raw_args = caps[2].to_string();
            let evaluated = match name {
                "hmac_sha256" => Self::eval_hmac_sha256(&raw_args, context),
                "sha256" => Self::eval_sha256(&raw_args, context),
                "random_int" => Self::eval_random_int(&raw_args, context),
                _ => None, // 未知函数宏：保持字面量，残留检查显式告警
            };
            evaluated.unwrap_or_else(|| caps[0].to_string())
        });

        // 通道 2：简单变量
        let re =
            VAR_REGEX.get_or_init(|| Regex::new(r"\{\{([$a-zA-Z_][a-zA-Z0-9_.]*)\}\}").unwrap());

        re.replace_all(&with_fns, |caps: &Captures| {
            let var_name = &caps[1];

            // 处理动态内置变量
            match var_name {
                "$uuid" => uuid::Uuid::new_v4().to_string(),
                "$timestamp" => chrono::Utc::now().timestamp().to_string(),
                // T1：保形态时间戳（ISO8601 / epoch 毫秒），生成的用例重跑语义正确
                "$timestamp_iso" => {
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                }
                "$timestamp_ms" => chrono::Utc::now().timestamp_millis().to_string(),
                "$random_int" => {
                    use rand::Rng;
                    let mut rng = rand::rng();
                    rng.random_range(1..=10000).to_string()
                }
                _ => context.get(var_name).unwrap_or_else(|| caps[0].to_string()),
            }
        })
        .to_string()
    }

    /// `$hmac_sha256(key, msg)` → 小写十六进制 HMAC-SHA256 摘要
    fn eval_hmac_sha256(raw_args: &str, context: &VariableContext) -> Option<String> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let args = Self::split_macro_args(raw_args);
        if args.len() != 2 {
            return None;
        }
        let key = Self::substitute(&args[0], context);
        let msg = Self::substitute(&args[1], context);

        let mut mac = HmacSha256::new_from_slice(key.as_bytes()).ok()?;
        mac.update(msg.as_bytes());
        Some(Self::hex_lower(&mac.finalize().into_bytes()))
    }

    /// `$sha256(msg)` → 小写十六进制 SHA-256 摘要
    fn eval_sha256(raw_args: &str, context: &VariableContext) -> Option<String> {
        use sha2::{Digest, Sha256};

        let args = Self::split_macro_args(raw_args);
        if args.len() != 1 {
            return None;
        }
        let msg = Self::substitute(&args[0], context);
        let mut hasher = Sha256::new();
        hasher.update(msg.as_bytes());
        Some(Self::hex_lower(&hasher.finalize()))
    }

    /// `$random_int(min, max)` → 闭区间 [min, max] 内的随机整数
    fn eval_random_int(raw_args: &str, context: &VariableContext) -> Option<String> {
        use rand::Rng;
        let args = Self::split_macro_args(raw_args);
        if args.len() != 2 {
            return None;
        }
        let min: i64 = Self::substitute(&args[0], context).trim().parse().ok()?;
        let max: i64 = Self::substitute(&args[1], context).trim().parse().ok()?;
        if min > max {
            return None;
        }
        let mut rng = rand::rng();
        Some(rng.random_range(min..=max).to_string())
    }

    /// 引号感知的宏参数分词器：顶层逗号分参，支持 `'...'` / `"..."` 与 `\` 转义，
    /// 严禁裸 `split(',')` 撕裂含逗号/空格的参数值（04 套件 5 契约）。
    fn split_macro_args(raw: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut cur = String::new();
        let mut in_quote: Option<char> = None;
        let mut escaped = false;
        for ch in raw.chars() {
            if escaped {
                cur.push(ch);
                escaped = false;
                continue;
            }
            match ch {
                '\\' if in_quote.is_some() => {
                    escaped = true;
                }
                q @ ('\'' | '"') if in_quote == Some(q) => {
                    in_quote = None;
                    cur.push(ch);
                }
                q @ ('\'' | '"') if in_quote.is_none() => {
                    in_quote = Some(q);
                    cur.push(ch);
                }
                ',' if in_quote.is_none() => {
                    args.push(cur.trim().to_string());
                    cur = String::new();
                }
                _ => cur.push(ch),
            }
        }
        if !cur.trim().is_empty() {
            args.push(cur.trim().to_string());
        }
        // 去除首尾配对引号
        args.into_iter()
            .map(|a| {
                let bytes = a.as_bytes();
                if bytes.len() >= 2
                    && (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'
                        || bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
                {
                    a[1..a.len() - 1].to_string()
                } else {
                    a
                }
            })
            .collect()
    }

    fn hex_lower(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// 解析并替换系统环境变量 ${VAR}
    pub fn resolve_env_vars(text: &str) -> String {
        static ENV_REGEX: OnceLock<Regex> = OnceLock::new();
        let re = ENV_REGEX.get_or_init(|| Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}").unwrap());

        re.replace_all(text, |caps: &Captures| {
            let env_name = &caps[1];
            std::env::var(env_name).unwrap_or_else(|_| caps[0].to_string())
        })
        .to_string()
    }

    /// 完整的变量解析流程：先解析环境变量，再替换自定义变量
    pub fn resolve(text: &str, context: &VariableContext) -> String {
        let with_env = Self::resolve_env_vars(text);
        Self::substitute(&with_env, context)
    }

    /// 检查渲染后的请求中是否仍残留 `{{...}}` 占位符（通常为未定义变量）。
    /// 返回去重后的警告信息；未定义变量此前会静默按字面量发送导致假通过。
    pub fn unresolved_placeholders(parsed: &crate::parser::ParsedRequest) -> Vec<String> {
        static RESIDUE_REGEX: OnceLock<Regex> = OnceLock::new();
        let re = RESIDUE_REGEX.get_or_init(|| Regex::new(r"\{\{[^}]*\}\}").unwrap());

        let mut found = std::collections::BTreeSet::new();
        let mut scan = |s: &str| {
            for m in re.find_iter(s) {
                found.insert(m.as_str().to_string());
            }
        };
        scan(&parsed.url);
        for (_key, value) in &parsed.headers {
            scan(value);
        }
        if let Some(body) = &parsed.body {
            scan(body);
        }

        found
            .into_iter()
            .map(|p| format!("变量 {p} 未定义，已按字面量发送"))
            .collect()
    }

    /// 渲染并实例化一个 `ParsedRequest`：解析其 URL, Headers, Body 中的所有占位符和环境变量；
    /// 并处理 Base URL 的自动拼接以及全局 User-Agent 的注入。
    pub fn resolve_parsed_request(
        parsed: &mut crate::parser::ParsedRequest,
        context: &mut VariableContext,
    ) {
        // 1. 替换 URL
        parsed.url = Self::resolve(&parsed.url, context);

        // 2. 替换 Headers
        for (_key, value) in &mut parsed.headers {
            *value = Self::resolve(value, context);
        }

        // 3. 检查全局变量 context 中是否提供了 user_agent，如果是且请求中没有显式设置，则追加
        if let Some(ua) = context.get("user_agent") {
            let has_ua = parsed
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"));
            if !has_ua {
                parsed.headers.push(("User-Agent".to_string(), ua));
            }
        }

        // 4. 替换 Body
        if let Some(body) = &mut parsed.body {
            *body = Self::resolve(body, context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unresolved_placeholders_detected() {
        // 回归：未定义变量此前静默按字面量发送，服务器 200 即假通过
        let mut parsed = crate::parser::ParsedRequest::new(1);
        parsed.url = "/users/{{good_id}}/x/{{good_id}}".to_string();
        parsed.headers = vec![("X-A".to_string(), "{{missing_a}}".to_string())];
        parsed.body = Some("{\"k\": \"{{also_missing}}\"}".to_string());

        let warnings = VariableResolver::unresolved_placeholders(&parsed);
        // 同一占位符出现多次只报一次
        assert_eq!(warnings.len(), 3);
        assert!(warnings.iter().any(|w| w.contains("{{good_id}}")));
        assert!(warnings.iter().any(|w| w.contains("{{missing_a}}")));
        assert!(warnings.iter().any(|w| w.contains("{{also_missing}}")));

        // 已解析完成的请求不应产生警告
        let clean = crate::parser::ParsedRequest::new(1);
        assert!(VariableResolver::unresolved_placeholders(&clean).is_empty());
    }

    #[test]
    fn test_substitute_simple() {
        let mut ctx = VariableContext::new();
        ctx.insert("base_url", "http://localhost:8080");
        ctx.insert("token", "secret-token");

        let input = "{{base_url}}/api/users";
        let output = VariableResolver::substitute(input, &ctx);
        assert_eq!(output, "http://localhost:8080/api/users");
    }

    #[test]
    fn test_substitute_multiple() {
        let mut ctx = VariableContext::new();
        ctx.insert("host", "example.com");
        ctx.insert("port", "8080");
        ctx.insert("path", "api");

        let input = "https://{{host}}:{{port}}/{{path}}/users";
        let output = VariableResolver::substitute(input, &ctx);
        assert_eq!(output, "https://example.com:8080/api/users");
    }

    #[test]
    fn test_substitute_missing_variable() {
        let ctx = VariableContext::new();

        let input = "{{missing}}/path";
        let output = VariableResolver::substitute(input, &ctx);
        // 未找到的变量保持原样
        assert_eq!(output, "{{missing}}/path");
    }

    #[test]
    fn test_resolve_env_vars() {
        // 设置测试环境变量
        unsafe {
            std::env::set_var("TEST_VAR", "test_value");
        }

        let input = "Value: ${TEST_VAR}";
        let output = VariableResolver::resolve_env_vars(input);
        assert_eq!(output, "Value: test_value");

        // 清理
        unsafe {
            std::env::remove_var("TEST_VAR");
        }
    }

    #[test]
    fn test_resolve_env_vars_missing() {
        let input = "Value: ${NONEXISTENT_VAR}";
        let output = VariableResolver::resolve_env_vars(input);
        // 未找到的环境变量保持原样
        assert_eq!(output, "Value: ${NONEXISTENT_VAR}");
    }

    #[test]
    fn test_resolve_combined() {
        unsafe {
            std::env::set_var("API_KEY", "secret-key");
        }

        let mut ctx = VariableContext::new();
        ctx.insert("host", "api.example.com");

        let input = "https://{{host}}/auth?key=${API_KEY}";
        let output = VariableResolver::resolve(input, &ctx);
        assert_eq!(output, "https://api.example.com/auth?key=secret-key");

        unsafe {
            std::env::remove_var("API_KEY");
        }
    }

    #[test]
    fn test_resolve_builtin_uuid() {
        let ctx = VariableContext::new();
        let input = "ID: {{$uuid}}";
        let output1 = VariableResolver::resolve(input, &ctx);
        let output2 = VariableResolver::resolve(input, &ctx);

        assert!(output1.starts_with("ID: "));
        assert_eq!(output1.len(), 4 + 36); // "ID: " + 36 char UUID
        // 验证两次生成的 UUID 不同
        assert_ne!(output1, output2);
    }

    #[test]
    fn test_resolve_builtin_timestamp() {
        let ctx = VariableContext::new();
        let input = "Time: {{$timestamp}}";
        let output = VariableResolver::resolve(input, &ctx);

        assert!(output.starts_with("Time: "));
        // 简单验证时间戳长度 (例如 1714543200 是 10 位)
        assert!(output.len() >= 14);
    }

    #[test]
    fn test_resolve_builtin_random_int() {
        let ctx = VariableContext::new();
        let input = "Rand: {{$random_int}}";
        let output = VariableResolver::resolve(input, &ctx);

        assert!(output.starts_with("Rand: "));
        let num_str = &output[6..];
        let num: u32 = num_str.parse().unwrap();
        assert!((1..=10000).contains(&num));
    }

    // ===== 函数宏（M1 内置加签宏，01 §1.2 [待建 M1]）=====

    /// RFC 4231 Test Case 2（ASCII key="Jefe"）—— 字符串宏可直接对齐官方向量
    #[test]
    fn test_hmac_sha256_rfc4231_case2() {
        let ctx = VariableContext::new();
        let output = VariableResolver::resolve(
            r#"{{$hmac_sha256("Jefe", "what do ya want for nothing?")}}"#,
            &ctx,
        );
        assert_eq!(
            output,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    /// 嵌套插值先行求值：ctx 提供的片段拼出 RFC 4231 Case 2 的 key，证明宏参数
    /// 在求值前完成了 `{{...}}` 解析（求值序守护测试）
    #[test]
    fn test_hmac_sha256_nested_interpolation() {
        let mut ctx = VariableContext::new();
        ctx.insert("k1", "Je");
        ctx.insert("k2", "fe");
        let output = VariableResolver::resolve(
            r#"{{$hmac_sha256("{{k1}}{{k2}}", "what do ya want for nothing?")}}"#,
            &ctx,
        );
        assert_eq!(
            output,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    /// 含逗号与空格的引号参数不被撕裂（引号感知分词器契约）
    #[test]
    fn test_hmac_sha256_quoted_comma_arg() {
        let ctx = VariableContext::new();
        let output = VariableResolver::resolve(r#"{{$sha256("a,b c")}}"#, &ctx);
        // SHA-256("a,b c")
        assert_eq!(
            output,
            "8e60a689618d2ebf84591742943652e2d4ba87b3bcac87df4450dce88701687b"
        );
    }

    #[test]
    fn test_sha256_known_vectors() {
        let ctx = VariableContext::new();
        assert_eq!(
            VariableResolver::resolve(r#"{{$sha256("abc")}}"#, &ctx),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            VariableResolver::resolve(r#"{{$sha256("")}}"#, &ctx),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_random_int_param_bounds() {
        let ctx = VariableContext::new();
        assert_eq!(
            VariableResolver::resolve("{{$random_int(5, 5)}}", &ctx),
            "5"
        );
        for _ in 0..32 {
            let out = VariableResolver::resolve("{{$random_int( 1 , 3 )}}", &ctx);
            let n: i64 = out.parse().unwrap();
            assert!((1..=3).contains(&n));
        }
    }

    /// 反静默：参数个数/取值非法时保留字面量，交由残留占位符检查显式告警
    #[test]
    fn test_function_macro_invalid_args_keeps_literal() {
        let ctx = VariableContext::new();
        for bad in [
            "{{$hmac_sha256(\"only-key\")}}",
            "{{$sha256(\"a\", \"b\")}}",
            "{{$random_int(abc, 3)}}",
            "{{$random_int(5, 1)}}",
            "{{$unknown_fn(1)}}",
        ] {
            assert_eq!(VariableResolver::resolve(bad, &ctx), bad, "case: {bad}");
            let mut parsed = crate::parser::ParsedRequest::new(1);
            parsed.url = bad.to_string();
            let warnings = VariableResolver::unresolved_placeholders(&parsed);
            assert!(!warnings.is_empty(), "非法函数宏必须被残留检查捕获: {bad}");
        }
    }

    #[test]
    fn test_resolve_global() {
        let mut ctx = VariableContext::new();
        ctx.insert("global.token", "global-secret");

        let input = "Bearer {{global.token}}";
        let output = VariableResolver::resolve(input, &ctx);
        assert_eq!(output, "Bearer global-secret");
    }

    #[test]
    fn test_resolve_env_via_placeholder() {
        unsafe {
            std::env::set_var("TEST_PLACEHOLDER", "env-value");
        }

        let ctx = VariableContext::new();
        let input = "Val: {{env.TEST_PLACEHOLDER}}";
        let output = VariableResolver::resolve(input, &ctx);
        assert_eq!(output, "Val: env-value");

        unsafe {
            std::env::remove_var("TEST_PLACEHOLDER");
        }
    }

    #[test]
    fn test_resolve_parsed_request() {
        let mut ctx = VariableContext::new();
        ctx.insert("host", "example.com");
        ctx.insert("user_id", "456");
        ctx.insert("base_url", "https://api.test.com/");
        ctx.insert("user_agent", "CustomTestAgent/1.0");

        let mut parsed = crate::parser::ParsedRequest::new(1);
        parsed.url = "/users/{{user_id}}".to_string();
        parsed.base_path = Some("v1".to_string());
        parsed.headers = vec![("Host".to_string(), "{{host}}".to_string())];
        parsed.body = Some("hello {{user_id}}".to_string());

        VariableResolver::resolve_parsed_request(&mut parsed, &mut ctx);

        // 验证 URL 渲染（此处不再自动拼接，仅替换占位符）
        assert_eq!(parsed.url, "/users/456");

        // 验证 Headers 渲染和 user_agent 自动注入
        assert_eq!(parsed.headers.len(), 2);
        assert_eq!(
            parsed.headers[0],
            ("Host".to_string(), "example.com".to_string())
        );
        assert_eq!(
            parsed.headers[1],
            ("User-Agent".to_string(), "CustomTestAgent/1.0".to_string())
        );

        // 验证 Body 渲染
        assert_eq!(parsed.body.unwrap(), "hello 456");
    }
}
