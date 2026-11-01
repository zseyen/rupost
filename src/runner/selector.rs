//! 单请求选择器：`--only <n|@name>` 与 `path#n` 片段的解析与应用（纯函数）。
//!
//! 设计裁决（DX-Debug PRD US-R1，2026-10-28）：采用"跳过标记"语义——
//! 选中请求正常执行，其余复用既有 `@skip` 机制标记跳过（不发包），
//! `request_number` 保留文件内原序号，与批量 JSON 报告及 E2/E5 的
//! `<file>#<request_number>` 标识格式保持可关联。

use crate::parser::types::ParsedFile;
use crate::{Result, RupostError};

/// 单请求选择器
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestSelector {
    /// 1-based 文件内序号
    Ordinal(usize),
    /// `@name` 元数据精确匹配（裸名等价；纯数字名需写 `@3`）
    Name(String),
}

/// `--only` 与 `path#n` 片段的合并结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorPlan {
    pub selector: Option<RequestSelector>,
    /// 片段拆分后的路径（无片段时原样返回）
    pub paths: Vec<String>,
}

/// 单请求选择的应用结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedSelection {
    /// 选中请求的文件内原序号（1-based）
    pub selected_number: usize,
    /// 被标记跳过的请求数
    pub skipped_count: usize,
    /// 选中请求的 `@name`（按序号选择时为 None）
    pub selected_name: Option<String>,
    /// 与选中同名的额外请求数（重名警告用）
    pub duplicate_names: usize,
}

/// 解析 `--only` 参数：纯数字 → 序号；`@name` / 裸名 → 名称
pub fn parse_only_selector(raw: &str) -> Result<RequestSelector> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(RupostError::Other(
            "--only 参数不能为空（用法：--only <序号> 或 --only @name）".to_string(),
        ));
    }
    if let Some(name) = trimmed.strip_prefix('@') {
        if name.is_empty() {
            return Err(RupostError::Other("--only @ 后缺少名称".to_string()));
        }
        return Ok(RequestSelector::Name(name.to_string()));
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        let n = trimmed
            .parse::<usize>()
            .map_err(|_| RupostError::Other(format!("--only 序号超出范围: {trimmed}")))?;
        return Ok(RequestSelector::Ordinal(n));
    }
    Ok(RequestSelector::Name(trimmed.to_string()))
}

/// 拆分路径片段：`api.http#3` → ("api.http", Some(3))。
///
/// 仅当最后一个 `#` 的后缀非空且全数字时视为选择器，否则整体按路径处理
/// （保护含 `#` 的真实文件名）；数字后缀超出 usize 时同样按文件名处理。
pub fn split_path_fragment(path: &str) -> (String, Option<usize>) {
    let Some((base, suffix)) = path.rsplit_once('#') else {
        return (path.to_string(), None);
    };
    let numeric = !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit());
    if numeric && let Ok(n) = suffix.parse::<usize>() {
        return (base.to_string(), Some(n));
    }
    (path.to_string(), None)
}

/// 从 paths 与 `--only` 构建选择器计划；校验互斥与"单一文件路径"约束。
pub fn build_selector_plan(paths: &[String], only: Option<&str>) -> Result<SelectorPlan> {
    let mut split = Vec::with_capacity(paths.len());
    let mut fragment: Option<(usize, usize)> = None;
    for (i, p) in paths.iter().enumerate() {
        let (base, frag) = split_path_fragment(p);
        if let Some(n) = frag {
            if fragment.is_some() {
                return Err(RupostError::Other(
                    "多个路径同时携带 #n 片段：片段选择器仅支持单一文件路径".to_string(),
                ));
            }
            fragment = Some((i, n));
        }
        split.push(base);
    }

    let selector = only.map(parse_only_selector).transpose()?;
    if selector.is_some() && fragment.is_some() {
        return Err(RupostError::Other(
            "--only 与 path#n 片段互斥，请二选一".to_string(),
        ));
    }
    if (selector.is_some() || fragment.is_some()) && paths.len() != 1 {
        return Err(RupostError::Other(
            "单请求选择器（--only / path#n）仅支持单一文件路径；若要用片段语法，请写作 api.http#3（# 附在路径上，勿作独立参数）".to_string(),
        ));
    }
    let selector = match (selector, fragment) {
        (Some(sel), _) => Some(sel),
        (None, Some((_, n))) => Some(RequestSelector::Ordinal(n)),
        (None, None) => None,
    };
    Ok(SelectorPlan {
        selector,
        paths: split,
    })
}

/// 将选择器应用到已解析文件：选中者保持原样，其余标记 `metadata.skip = true`
/// （复用既有 `@skip` 机制，executor 产出 skipped 结果并保留原 request_number）。
pub fn apply_selector(
    file: &mut ParsedFile,
    selector: &RequestSelector,
) -> Result<AppliedSelection> {
    let total = file.requests.len();
    if total == 0 {
        return Err(RupostError::Other("文件内没有可执行的请求".to_string()));
    }
    match selector {
        RequestSelector::Ordinal(n) => {
            if *n == 0 || *n > total {
                return Err(RupostError::Other(format!(
                    "序号 {n} 越界：文件共 {total} 条请求，合法范围 1–{total}"
                )));
            }
            for (idx, req) in file.requests.iter_mut().enumerate() {
                req.metadata.skip = idx + 1 != *n;
            }
            Ok(AppliedSelection {
                selected_number: *n,
                skipped_count: total - 1,
                selected_name: None,
                duplicate_names: 0,
            })
        }
        RequestSelector::Name(want) => {
            let matches: Vec<usize> = file
                .requests
                .iter()
                .enumerate()
                .filter(|(_, r)| r.name() == Some(want.as_str()))
                .map(|(i, _)| i + 1)
                .collect();
            if matches.is_empty() {
                let available: Vec<&str> = file.requests.iter().filter_map(|r| r.name()).collect();
                let hint = if available.is_empty() {
                    "文件内没有任何 @name 元数据".to_string()
                } else {
                    format!("文件内可用的 @name: {}", available.join(", "))
                };
                return Err(RupostError::Other(format!(
                    "未找到名为 \"{want}\" 的请求（{hint}）"
                )));
            }
            for (idx, req) in file.requests.iter_mut().enumerate() {
                req.metadata.skip = idx + 1 != matches[0];
            }
            Ok(AppliedSelection {
                selected_number: matches[0],
                skipped_count: total - 1,
                selected_name: Some(want.clone()),
                duplicate_names: matches.len() - 1,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    fn file_from(text: &str) -> ParsedFile {
        parser::parse_content(text).expect("parse fixture failed")
    }

    // ---- parse_only_selector 决策表 ----

    #[test]
    fn test_parse_only_selector_decision_table() {
        assert_eq!(
            parse_only_selector("3").unwrap(),
            RequestSelector::Ordinal(3)
        );
        assert_eq!(
            parse_only_selector(" 12 ").unwrap(),
            RequestSelector::Ordinal(12)
        );
        assert_eq!(
            parse_only_selector("0").unwrap(),
            RequestSelector::Ordinal(0)
        );
        assert_eq!(
            parse_only_selector("@login").unwrap(),
            RequestSelector::Name("login".to_string())
        );
        assert_eq!(
            parse_only_selector("login").unwrap(),
            RequestSelector::Name("login".to_string())
        );
        // 纯数字名需写 @3
        assert_eq!(
            parse_only_selector("@3").unwrap(),
            RequestSelector::Name("3".to_string())
        );
        assert!(parse_only_selector("").is_err());
        assert!(parse_only_selector("@").is_err());
        assert!(parse_only_selector("   ").is_err());
    }

    // ---- split_path_fragment 决策表 ----

    #[test]
    fn test_split_path_fragment_decision_table() {
        assert_eq!(
            split_path_fragment("api.http#3"),
            ("api.http".to_string(), Some(3))
        );
        assert_eq!(
            split_path_fragment("api.http"),
            ("api.http".to_string(), None)
        );
        // 非数字后缀 → 整体按文件名（保护含 # 的真实文件）
        assert_eq!(
            split_path_fragment("api#3.http"),
            ("api#3.http".to_string(), None)
        );
        // 多个 # 取最后一个
        assert_eq!(split_path_fragment("a#b#2"), ("a#b".to_string(), Some(2)));
        // 目录 + 片段照样拆分（目录拒绝在应用层）
        assert_eq!(split_path_fragment("dir/#2"), ("dir/".to_string(), Some(2)));
        // 溢出数字 → 整体按文件名
        assert_eq!(
            split_path_fragment("x#99999999999999999999999"),
            ("x#99999999999999999999999".to_string(), None)
        );
    }

    // ---- build_selector_plan 决策表 ----

    #[test]
    fn test_build_selector_plan_decision_table() {
        // 无选择器：路径原样透传
        let plan = build_selector_plan(&["a.http".to_string()], None).unwrap();
        assert_eq!(plan.selector, None);
        assert_eq!(plan.paths, vec!["a.http".to_string()]);

        // 片段拆分
        let plan = build_selector_plan(&["api.http#3".to_string()], None).unwrap();
        assert_eq!(plan.selector, Some(RequestSelector::Ordinal(3)));
        assert_eq!(plan.paths, vec!["api.http".to_string()]);

        // 互斥
        let err = build_selector_plan(&["api.http#3".to_string()], Some("3")).unwrap_err();
        assert!(err.to_string().contains("互斥"));

        // 多路径 + 选择器
        let err = build_selector_plan(&["a.http".to_string(), "b.http".to_string()], Some("1"))
            .unwrap_err();
        assert!(err.to_string().contains("单一文件路径"));

        // 多路径片段
        let err =
            build_selector_plan(&["a.http#1".to_string(), "b.http".to_string()], None).unwrap_err();
        assert!(err.to_string().contains("单一文件路径"));
    }

    // ---- apply_selector 决策表 ----

    #[test]
    fn test_apply_selector_ordinal_marks_skip_and_keeps_number() {
        let mut file = file_from("GET http://x/1\n\nGET http://x/2\n\nGET http://x/3\n");
        let applied = apply_selector(&mut file, &RequestSelector::Ordinal(2)).unwrap();
        assert_eq!(applied.selected_number, 2);
        assert_eq!(applied.skipped_count, 2);
        assert!(file.requests[0].metadata.skip);
        assert!(!file.requests[1].metadata.skip);
        assert!(file.requests[2].metadata.skip);
    }

    #[test]
    fn test_apply_selector_ordinal_range_errors() {
        let mut file = file_from("GET http://x/1\n\nGET http://x/2\n");
        for bad in [0usize, 3] {
            let err = apply_selector(&mut file, &RequestSelector::Ordinal(bad)).unwrap_err();
            assert!(err.to_string().contains("合法范围"), "got: {err}");
        }
    }

    #[test]
    fn test_apply_selector_name_first_match_and_duplicates() {
        // 注意：`@name` 是请求块内的注释元数据行；`###` 是分隔符
        let text = "###\n# @name login\nGET http://x/1\n\nGET http://x/2\n\n###\n# @name login\nGET http://x/3\n";
        let mut file = file_from(text);
        let applied = apply_selector(&mut file, &RequestSelector::Name("login".into())).unwrap();
        // 重名取首个
        assert_eq!(applied.selected_number, 1);
        assert_eq!(applied.duplicate_names, 1);
        assert!(!file.requests[0].metadata.skip);
        assert!(file.requests[1].metadata.skip);
        assert!(file.requests[2].metadata.skip);
    }

    #[test]
    fn test_apply_selector_name_not_found_lists_available() {
        let text = "###\n# @name login\nGET http://x/1\n\n###\n# @name logout\nGET http://x/2\n";
        let mut file = file_from(text);
        let err = apply_selector(&mut file, &RequestSelector::Name("nosuch".into())).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("未找到名为"), "got: {msg}");
        assert!(
            msg.contains("login") && msg.contains("logout"),
            "got: {msg}"
        );
    }

    #[test]
    fn test_apply_selector_name_without_any_metadata() {
        let mut file = file_from("GET http://x/1\n");
        let err = apply_selector(&mut file, &RequestSelector::Name("x".into())).unwrap_err();
        assert!(err.to_string().contains("没有任何 @name"));
    }
}
