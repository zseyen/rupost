pub struct JsonPathResolver;

/// 将 JSONPath 路径（支持 $.a[0].b, a.b, a.0.b 等形式）统一转换为扁平的 segment 字符串向量
pub fn parse_jsonpath_to_segments(path: &str) -> Vec<String> {
    let mut path = path.trim();
    if path.is_empty() {
        return Vec::new();
    }

    // 剥离 $ 或 $. 前缀
    if path.starts_with('$') {
        path = &path[1..];
        if path.starts_with('.') {
            path = &path[1..];
        }
    }

    let mut segments = Vec::new();
    let mut current_segment = String::new();
    let chars: Vec<char> = path.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '.' {
            if !current_segment.is_empty() {
                segments.push(current_segment.clone());
                current_segment.clear();
            }
        } else if c == '[' {
            if !current_segment.is_empty() {
                segments.push(current_segment.clone());
                current_segment.clear();
            }
            i += 1;
            let mut bracket_content = String::new();
            while i < chars.len() && chars[i] != ']' {
                bracket_content.push(chars[i]);
                i += 1;
            }
            let trimmed_content = bracket_content.trim().trim_matches('\'').trim_matches('"');
            if !trimmed_content.is_empty() {
                segments.push(trimmed_content.to_string());
            }
        } else {
            current_segment.push(c);
        }
        i += 1;
    }

    if !current_segment.is_empty() {
        segments.push(current_segment);
    }

    segments
}

impl JsonPathResolver {
    /// 利用预编译的 segments 进行求值，避免重复解析开销
    pub fn resolve_with_segments<'a>(val: &'a serde_json::Value, segments: &[String]) -> Option<&'a serde_json::Value> {
        let mut current = val;
        for seg in segments {
            if let (serde_json::Value::Array(arr), Ok(idx)) = (current, seg.parse::<usize>()) {
                current = arr.get(idx)?;
                continue;
            }
            current = current.get(seg)?;
        }
        Some(current)
    }

    /// 统一的值解析逻辑
    pub fn resolve<'a>(val: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
        let segments = parse_jsonpath_to_segments(path);
        Self::resolve_with_segments(val, &segments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_jsonpath_to_segments() {
        // 基础字段定位
        assert_eq!(parse_jsonpath_to_segments("a.b.c"), vec!["a", "b", "c"]);
        assert_eq!(parse_jsonpath_to_segments("$.a.b.c"), vec!["a", "b", "c"]);

        // 中括号索引
        assert_eq!(parse_jsonpath_to_segments("a[0].b"), vec!["a", "0", "b"]);
        assert_eq!(
            parse_jsonpath_to_segments("$.a[10].b"),
            vec!["a", "10", "b"]
        );

        // 点号数字索引
        assert_eq!(parse_jsonpath_to_segments("a.0.b"), vec!["a", "0", "b"]);
        assert_eq!(parse_jsonpath_to_segments("$.a.99.b"), vec!["a", "99", "b"]);

        // 根数组索引
        assert_eq!(parse_jsonpath_to_segments("$[0]"), vec!["0"]);
        assert_eq!(parse_jsonpath_to_segments("$[1].name"), vec!["1", "name"]);

        // 中括号引号剥离与空格支持
        assert_eq!(
            parse_jsonpath_to_segments("a['first name']"),
            vec!["a", "first name"]
        );
        assert_eq!(
            parse_jsonpath_to_segments("$.a[\"last name\"]"),
            vec!["a", "last name"]
        );

        // 空路径
        assert_eq!(parse_jsonpath_to_segments(""), Vec::<String>::new());
        assert_eq!(parse_jsonpath_to_segments("$"), Vec::<String>::new());
        assert_eq!(parse_jsonpath_to_segments("$."), Vec::<String>::new());
    }

    #[test]
    fn test_json_path_resolver_resolve() {
        let doc = json!({
            "store": {
                "books": [
                    { "title": "Book 1", "price": 10.0 },
                    { "title": "Book 2", "price": 20.0 }
                ],
                "address": {
                    "city": "Tokyo"
                }
            },
            "tags": ["tech", "rust"],
            "matrix": [
                [1, 2],
                [3, 4]
            ]
        });

        // 基础解析
        assert_eq!(
            JsonPathResolver::resolve(&doc, "$.store.address.city"),
            Some(&json!("Tokyo"))
        );
        assert_eq!(
            JsonPathResolver::resolve(&doc, "store.address.city"),
            Some(&json!("Tokyo"))
        );

        // 数组中括号定位
        assert_eq!(
            JsonPathResolver::resolve(&doc, "$.store.books[0].title"),
            Some(&json!("Book 1"))
        );
        assert_eq!(
            JsonPathResolver::resolve(&doc, "store.books[1].price"),
            Some(&json!(20.0))
        );

        // 数组点号数字定位
        assert_eq!(
            JsonPathResolver::resolve(&doc, "$.store.books.0.title"),
            Some(&json!("Book 1"))
        );
        assert_eq!(
            JsonPathResolver::resolve(&doc, "store.books.1.price"),
            Some(&json!(20.0))
        );

        // 根数组定位
        let root_arr = json!([
            { "name": "Alice" },
            { "name": "Bob" }
        ]);
        assert_eq!(
            JsonPathResolver::resolve(&root_arr, "$[0].name"),
            Some(&json!("Alice"))
        );
        assert_eq!(
            JsonPathResolver::resolve(&root_arr, "[1].name"),
            Some(&json!("Bob"))
        );

        // 一维数组值定位
        assert_eq!(
            JsonPathResolver::resolve(&doc, "$.tags[1]"),
            Some(&json!("rust"))
        );
        assert_eq!(
            JsonPathResolver::resolve(&doc, "tags.0"),
            Some(&json!("tech"))
        );

        // 多维数组
        assert_eq!(
            JsonPathResolver::resolve(&doc, "$.matrix[1][0]"),
            Some(&json!(3))
        );
        assert_eq!(
            JsonPathResolver::resolve(&doc, "matrix.0.1"),
            Some(&json!(2))
        );

        // 未命中路径
        assert_eq!(JsonPathResolver::resolve(&doc, "$.store.nonexistent"), None);
        assert_eq!(JsonPathResolver::resolve(&doc, "$.store.books[2]"), None);
        assert_eq!(
            JsonPathResolver::resolve(&doc, "$.store.books.0.nonexistent"),
            None
        );
    }
}
