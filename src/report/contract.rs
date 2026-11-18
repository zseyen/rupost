//! 契约漂移基线（WP-P2b）：只记录确定性字段（断言原文 + 通过与否），动态值
//! 从根上不进摘要；diff 以 `id::assertion` 为键，断言原文改变视同新增。
use crate::runner::types::TestResult;
use crate::utils::path_guard::normalize_posix_path;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub const CONTRACT_SCHEMA_VERSION: &str = "1.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractEntry {
    pub id: String,
    pub assertion: String,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ContractSummary {
    pub schema_version: String,
    pub contracts: Vec<ContractEntry>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContractDrift {
    pub newly_broken: usize,
    pub fixed: usize,
    pub new: usize,
    /// 基线有、本次缺失（服务端宕机全错运行/请求删除/文件改名）——
    /// 审查 P2 修正：静默消失必须可见，防"空运行覆写基线"侵蚀漂移信号
    pub removed: usize,
}

/// 批处理键（canonicalize 后的绝对路径）→ 相对展示路径：剥 cwd 前缀
/// （含 macOS /var→/private/var 符号链接形态）。基线文件可能被提交进仓库，
/// id 必须跨机器稳定；已相对或无法剥离时原样返回。
fn display_path(path: &std::path::Path) -> String {
    let s = normalize_posix_path(path);
    if let Ok(cwd) = std::env::current_dir() {
        let mut bases = vec![normalize_posix_path(&cwd)];
        if let Ok(canon) = std::fs::canonicalize(&cwd) {
            bases.push(normalize_posix_path(&canon));
        }
        for base in bases {
            if !base.is_empty()
                && let Some(rest) = s.strip_prefix(&format!("{base}/"))
            {
                return rest.to_string();
            }
        }
    }
    // 审查 P3：剥离失败 = id 将以绝对路径入基线（跨机器/目录复跑全部变 new，
    // 可移植性退化）——显式提示，不静默
    tracing::warn!("契约 id 无法相对化（将写入绝对路径）: {s}");
    s
}

/// 稳定 ID：<path>#L<行号>；无行号退化 <path>#<序号>
pub fn entry_id(path: &std::path::Path, result: &TestResult) -> String {
    let display = display_path(path);
    match result.source_line {
        Some(line) => format!("{display}#L{line}"),
        None => format!("{display}#{}", result.request_number),
    }
}

pub fn build_contract_summary(
    batch_results: &[(std::path::PathBuf, Vec<TestResult>)],
) -> ContractSummary {
    let mut contracts = Vec::new();
    for (path, results) in batch_results {
        for r in results {
            if r.is_teardown {
                continue;
            }
            for a in &r.assertions {
                contracts.push(ContractEntry {
                    id: entry_id(path, r),
                    assertion: a.raw.clone(),
                    passed: a.passed,
                });
            }
        }
    }
    ContractSummary {
        schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
        contracts,
    }
}

/// diff：键 = id::assertion（断言原文改变视同新增）
pub fn diff_contracts(baseline: &ContractSummary, current: &ContractSummary) -> ContractDrift {
    let key = |e: &ContractEntry| format!("{}::{}", e.id, e.assertion);
    let base: HashMap<String, bool> = baseline
        .contracts
        .iter()
        .map(|e| (key(e), e.passed))
        .collect();
    let mut drift = ContractDrift::default();
    for e in &current.contracts {
        match base.get(&key(e)) {
            Some(true) if !e.passed => drift.newly_broken += 1,
            Some(false) if e.passed => drift.fixed += 1,
            None => drift.new += 1,
            _ => {}
        }
    }
    // 审查 P2 修正：基线有、本次缺失 = removed（防静默侵蚀）
    for e in &baseline.contracts {
        if !current.contracts.iter().any(|c| key(c) == key(e)) {
            drift.removed += 1;
        }
    }
    drift
}

pub fn save_baseline(path: &Path, summary: &ContractSummary) -> crate::Result<()> {
    let bytes = serde_json::to_vec_pretty(summary)?;
    crate::security::fs::write_private_file_atomic(path, &bytes)
}

pub fn load_baseline(path: &Path) -> crate::Result<Option<ContractSummary>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(crate::error::RupostError::IoError(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, assertion: &str, passed: bool) -> ContractEntry {
        ContractEntry {
            id: id.into(),
            assertion: assertion.into(),
            passed,
        }
    }

    /// 表驱动：broken / fixed / new / unchanged / 断言原文改变五类漂移
    #[test]
    fn test_diff_contracts_table() {
        let mk = |items: &[(&str, &str, bool)]| ContractSummary {
            schema_version: CONTRACT_SCHEMA_VERSION.into(),
            contracts: items.iter().map(|(i, a, p)| entry(i, a, *p)).collect(),
        };
        let cases = [
            (
                mk(&[("a#L1", "status == 200", true)]),
                mk(&[("a#L1", "status == 200", false)]),
                (1, 0, 0),
                "newly broken",
            ),
            (
                mk(&[("a#L1", "status == 200", false)]),
                mk(&[("a#L1", "status == 200", true)]),
                (0, 1, 0),
                "fixed",
            ),
            (
                mk(&[]),
                mk(&[("a#L1", "status == 200", true)]),
                (0, 0, 1),
                "new",
            ),
            (
                mk(&[
                    ("a#L1", "status == 200", true),
                    ("b#L2", "status == 404", false),
                ]),
                mk(&[
                    ("a#L1", "status == 200", true),
                    ("b#L2", "status == 404", false),
                ]),
                (0, 0, 0),
                "unchanged",
            ),
            // 断言原文改变视同新增
            (
                mk(&[("a#L1", "status == 200", true)]),
                mk(&[("a#L1", "status == 201", true)]),
                (0, 0, 1),
                "assertion text changed counts as new",
            ),
        ];
        for (base, current, expected, name) in cases {
            let d = diff_contracts(&base, &current);
            assert_eq!(
                (d.newly_broken, d.fixed, d.new),
                (expected.0, expected.1, expected.2),
                "{name}"
            );
        }

        // 审查 P2 修正案例：基线有、本次缺失 = removed（静默消失可见化）
        let base = mk(&[
            ("a#L1", "status == 200", true),
            ("b#L2", "status == 404", false),
        ]);
        let current = mk(&[("a#L1", "status == 200", true)]);
        let d = diff_contracts(&base, &current);
        assert_eq!(
            (d.newly_broken, d.fixed, d.new, d.removed),
            (0, 0, 0, 1),
            "missing entry must surface as removed"
        );
    }

    /// schema 精确集：顶层两键、条目三键、无 actual 值
    #[test]
    fn test_summary_schema_exact_keys() {
        let summary = ContractSummary {
            schema_version: CONTRACT_SCHEMA_VERSION.into(),
            contracts: vec![entry("ok.http#L3", "status == 200", true)],
        };
        let v: serde_json::Value = serde_json::to_value(&summary).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 2);
        assert!(obj.contains_key("schema_version") && obj.contains_key("contracts"));
        assert_eq!(obj["contracts"][0].as_object().unwrap().len(), 3);
        assert_eq!(v["schema_version"], "1.0");
    }

    #[test]
    fn test_entry_id_line_and_fallback() {
        let mut r = TestResult::error(
            1,
            None,
            "GET".into(),
            "http://x".into(),
            "e".into(),
            std::time::Duration::from_millis(1),
        );
        let id = entry_id(std::path::Path::new("a.http"), &r);
        assert_eq!(id, "a.http#1", "无行号退化序号");
        r.source_line = Some(7);
        assert_eq!(entry_id(std::path::Path::new("a.http"), &r), "a.http#L7");
    }
}
