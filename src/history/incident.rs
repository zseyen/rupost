//! 故障复现包（Incident Bundle）领域工件（差异化执行计划 WP-3，Phase 1 主线）。
//!
//! 把既有高价值资产——掩码快照（`save_batch_snapshot`）、JSON 报告
//! （`report::json`）、安全写入原语（`security::fs`）——收束为一个稳定、
//! 脱敏、可重放、可移交的单一交付物；绝不重新执行请求、绝不复制源
//! Markdown/`.http`（避免字面凭据二次落盘）。
//!
//! 依赖边界：本模块属历史/工件层，可以依赖领域数据、报告 JSON 与安全原语；
//! 不得依赖 `cli`、`web`、`tui`。

use crate::Result;
use crate::history::model::MaskingMode;
use crate::history::snapshot_io::save_batch_snapshot;
use crate::runner::types::TestResult;
use crate::security::fs::{publish_private_dir_atomically, write_private_file_atomic};
use crate::utils::path_guard::normalize_posix_path;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 工件完整性摘要（WP-R6）：只防传输损坏/无意篡改；防恶意篡改需密钥签名（登记不做）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactDigest {
    pub file: String,
    /// 64 位小写 hex sha256（编码写法复用 snapshot_io 的 format!("{b:02x}") 模式）
    pub sha256: String,
    pub bytes: u64,
}

/// 复现包 manifest 契约版本（Incident schema 1.0；只追加字段，不改字段语义）
pub const INCIDENT_SCHEMA_VERSION: &str = "1.0";

/// 复现包固定工件名集合（目录结构契约：三文件齐全方可交付）
pub struct IncidentArtifactNames;

impl IncidentArtifactNames {
    pub const MANIFEST: &'static str = "manifest.json";
    pub const REPORT: &'static str = "report.json";
    pub const SNAPSHOT: &'static str = "snapshot.json";
    /// 发布顺序无关；manifest 列出的固定数组（外部消费者据此校验完整性）
    pub const ALL: [&'static str; 3] = [Self::MANIFEST, Self::REPORT, Self::SNAPSHOT];
}

/// 复现包清单（Incident schema 1.0）：文件名、时间、来源、计数与脱敏声明。
///
/// 隐私契约：绝不写入变量值、Cookie、插件参数、绝对工作目录或 AI 输出——
/// `source_paths` 只承载选择器规整后的输入路径（经 `normalize_posix_path`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IncidentManifest {
    /// 契约版本，恒定 "1.0"
    pub schema_version: String,
    /// 工件生成时间（ISO 8601 UTC）
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 来源路径列表（选择器规整后的相对展示路径）
    pub source_paths: Vec<String>,
    /// 运行环境标识（--env 提供时非空）
    pub environment: Option<String>,
    /// 请求总数
    pub total: usize,
    /// 失败数（不含 skipped；与 CLI 报告口径一致：`!success && !skipped`）
    pub failed: usize,
    /// 跳过数（不计入失败）
    pub skipped: usize,
    /// 固定工件文件名数组
    pub artifacts: Vec<String>,
    /// 脱敏声明：复现包强制 `secrets`（键名与显式值双重掩码，无关闭开关）
    pub masking: MaskingMode,
    /// 快照响应体录入上限（字节）
    pub max_snapshot_body_bytes: u64,
    /// WP-R6：工件完整性摘要（report/snapshot 两文件；manifest 自身无法自含——
    /// 消费方语义：两工件摘要匹配 + manifest 存在即完整）
    #[serde(default)]
    pub artifact_digests: Vec<ArtifactDigest>,
}

/// WP-R6：sha256 → 64 位小写 hex（NIST 向量单测锚定编码，防"实现与测试同错互证"）
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 从批处理结果计算 (total, failed, skipped) 纯函数：
/// 与 CLI 报告/Web 套件聚合同口径——`skipped` 不计入失败分母。
pub fn failure_counts(batch_results: &[(PathBuf, Vec<TestResult>)]) -> (usize, usize, usize) {
    let mut total = 0;
    let mut failed = 0;
    let mut skipped = 0;
    for (_, results) in batch_results {
        for r in results {
            total += 1;
            if r.skipped {
                skipped += 1;
            } else if !r.success {
                failed += 1;
            }
        }
    }
    (total, failed, skipped)
}

/// 从一次已完成的失败运行产出复现包目录（三文件齐全，整目录原子发布）。
///
/// 输入只接收已产生的 `batch_results`、`resolved_paths`、环境标识、显式
/// secret values、body 上限与目标路径——不重新执行任何请求。
///
/// 安全语义（WP-3 边界）：
/// - 快照强制掩码（`mask_secrets = true` + 显式值清单），manifest 声明
///   `masking = "secrets"`，绝不提供关闭脱敏的开关；
/// - 目标目录必须不存在（已存在/符号链接一律 `SecurityViolation`，拒绝覆盖）；
/// - 任一文件写入失败：最终目标不存在且无 staging 残留（fail-closed）；
/// - 包含整套非 teardown 交互（登录等依赖是回放的必要上下文，由调用方传入
///   的 `batch_results` 决定），不摘除"非失败请求"。
pub fn create_incident_bundle(
    batch_results: &[(PathBuf, Vec<TestResult>)],
    resolved_paths: &[String],
    environment: Option<&str>,
    secret_values: &[String],
    max_snapshot_body_bytes: usize,
    target_dir: &Path,
) -> Result<()> {
    let (total, failed, skipped) = failure_counts(batch_results);
    publish_private_dir_atomically(target_dir, |staging| {
        // 1. snapshot.json：复用既有黑匣子写入链路，强制掩码（Schema v1.0 不变）
        save_batch_snapshot(
            batch_results,
            resolved_paths,
            staging.join(IncidentArtifactNames::SNAPSHOT),
            true,
            secret_values,
            environment,
            max_snapshot_body_bytes,
        )?;

        // 2. report.json：复用 JSON 报告契约并应用显式 secret_values 脱敏（writer 注入式 → 缓冲 → 私有原子写入）
        let mut report_buf = Vec::new();
        crate::report::json::report_json_with_secrets(
            batch_results,
            secret_values,
            &mut report_buf,
        )?;
        write_private_file_atomic(&staging.join(IncidentArtifactNames::REPORT), &report_buf)?;

        // WP-R6：digests 先于 manifest 序列化计算（staging 字节 = 发布字节，rename 不改内容）
        let mut digests = Vec::new();
        for name in [
            IncidentArtifactNames::REPORT,
            IncidentArtifactNames::SNAPSHOT,
        ] {
            let bytes = std::fs::read(staging.join(name))?;
            digests.push(ArtifactDigest {
                file: name.to_string(),
                sha256: sha256_hex(&bytes),
                bytes: bytes.len() as u64,
            });
        }

        // 3. manifest.json 最后写（完整性声明以全部工件已落盘为前提）
        let manifest = IncidentManifest {
            schema_version: INCIDENT_SCHEMA_VERSION.to_string(),
            created_at: chrono::Utc::now(),
            source_paths: resolved_paths.iter().map(normalize_posix_path).collect(),
            environment: environment.map(str::to_string),
            total,
            failed,
            skipped,
            artifacts: IncidentArtifactNames::ALL
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            masking: MaskingMode::Secrets,
            max_snapshot_body_bytes: max_snapshot_body_bytes as u64,
            artifact_digests: digests,
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
        write_private_file_atomic(
            &staging.join(IncidentArtifactNames::MANIFEST),
            &manifest_bytes,
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::ExecutionSnapshotFile;
    use crate::history::snapshot_io::read_execution_snapshot;
    use std::time::Duration;
    use tempfile::TempDir;

    /// 表驱动锁定计数口径：空输入、纯失败、skipped、混合、多文件
    #[test]
    fn test_failure_counts_table() {
        fn fail(n: usize) -> TestResult {
            TestResult::error(
                n,
                None,
                "GET".into(),
                "http://x/y".into(),
                "boom".into(),
                Duration::from_millis(1),
            )
        }
        let skipped = |n: usize| TestResult::skipped(n, None, "GET".into(), "http://x/y".into());
        let ok = |n: usize| {
            TestResult::success(
                n,
                None,
                "GET".into(),
                "http://x/y".into(),
                crate::http::Response::new(
                    200,
                    reqwest::header::HeaderMap::new(),
                    String::new(),
                    Duration::from_millis(1),
                    Duration::ZERO,
                    Duration::ZERO,
                )
                .unwrap(),
            )
        };

        type Batch = Vec<(PathBuf, Vec<TestResult>)>;
        let empty: Batch = Vec::new();
        let cases: Vec<(Batch, (usize, usize, usize), &str)> = vec![
            (empty, (0, 0, 0), "empty"),
            (
                vec![(PathBuf::from("a.http"), vec![fail(1), fail(2)])],
                (2, 2, 0),
                "pure failures",
            ),
            (
                vec![(PathBuf::from("a.http"), vec![skipped(1)])],
                (1, 0, 1),
                "skipped not failed",
            ),
            (
                vec![(PathBuf::from("a.http"), vec![fail(1), skipped(2), ok(3)])],
                (3, 1, 1),
                "mixed",
            ),
            (
                vec![
                    (PathBuf::from("a.http"), vec![fail(1)]),
                    (PathBuf::from("b.http"), vec![ok(1), fail(2)]),
                ],
                (3, 2, 0),
                "multi file",
            ),
        ];
        for (input, expected, name) in cases {
            assert_eq!(failure_counts(&input), expected, "case: {name}");
        }
    }

    /// NIST FIPS-180 已知向量：锚定 hex 编码（独立于实现的算法/编码正确性证明，
    /// 消除"测试与实现共用 sha2 + 同编码写法"的同错互证盲区——子代理审查 P2）
    #[test]
    fn test_sha256_hex_nist_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// manifest 序列化契约：字段集合精确（只增不改），取值符合 schema 1.0
    #[test]
    fn test_manifest_schema_fields_exact_contract() {
        let manifest = IncidentManifest {
            schema_version: INCIDENT_SCHEMA_VERSION.to_string(),
            created_at: chrono::Utc::now(),
            source_paths: vec!["payments.http".to_string()],
            environment: None,
            total: 3,
            failed: 1,
            skipped: 1,
            artifacts: IncidentArtifactNames::ALL
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            masking: MaskingMode::Secrets,
            max_snapshot_body_bytes: 4096,
            artifact_digests: vec![ArtifactDigest {
                file: "snapshot.json".to_string(),
                sha256: "ab".repeat(32),
                bytes: 128,
            }],
        };
        let v: serde_json::Value = serde_json::to_value(&manifest).unwrap();
        let expected_keys = [
            "schema_version",
            "created_at",
            "source_paths",
            "environment",
            "total",
            "failed",
            "skipped",
            "artifacts",
            "masking",
            "max_snapshot_body_bytes",
            "artifact_digests",
        ];
        let obj = v.as_object().expect("manifest must serialize to object");
        assert_eq!(
            obj.len(),
            expected_keys.len(),
            "field set must match contract exactly: {:?}",
            obj.keys()
        );
        for k in expected_keys {
            assert!(obj.contains_key(k), "missing contract field: {k}");
        }
        assert_eq!(v["schema_version"], "1.0");
        assert_eq!(v["masking"], "secrets");
        assert_eq!(
            v["artifacts"],
            serde_json::json!(["manifest.json", "report.json", "snapshot.json"])
        );
        assert!(v["environment"].is_null());
        assert_eq!(v["max_snapshot_body_bytes"], 4096);
    }

    /// 工件名集合稳定契约（目录结构是外部契约，改动即破坏消费者）
    #[test]
    fn test_artifact_names_stable() {
        assert_eq!(
            IncidentArtifactNames::ALL,
            ["manifest.json", "report.json", "snapshot.json"]
        );
    }

    /// 端到端（领域层）：合成失败结果 → 三文件齐全、masking=secrets、
    /// snapshot 可解析为 Schema v1.0 且 secret 值被掩码
    #[test]
    fn test_create_incident_bundle_end_to_end() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("incident");

        let failing = TestResult::error(
            1,
            Some("charge".into()),
            "POST".into(),
            "http://127.0.0.1:1/charge".into(),
            "connection refused".into(),
            Duration::from_millis(5),
        );
        let batch = vec![(PathBuf::from("payments.http"), vec![failing])];
        let secret = "super-secret-token-xyz";

        create_incident_bundle(
            &batch,
            &["payments.http".to_string()],
            Some("staging"),
            std::slice::from_ref(&secret.to_string()),
            4096,
            &target,
        )
        .unwrap();

        // 三文件齐全 + 0600/0700
        for name in IncidentArtifactNames::ALL {
            assert!(target.join(name).is_file(), "missing artifact: {name}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700);
            let file_mode = std::fs::metadata(target.join("manifest.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600);
        }

        // manifest 契约
        let manifest_bytes = std::fs::read(target.join("manifest.json")).unwrap();
        let manifest: IncidentManifest = serde_json::from_slice(&manifest_bytes).unwrap();
        assert_eq!(manifest.schema_version, "1.0");
        assert_eq!(manifest.masking, MaskingMode::Secrets);
        assert_eq!(manifest.total, 1);
        assert_eq!(manifest.failed, 1);
        assert_eq!(manifest.skipped, 0);
        assert_eq!(manifest.environment.as_deref(), Some("staging"));
        assert_eq!(manifest.source_paths, vec!["payments.http"]);

        // snapshot 可被既有读取链路消费，且为掩码形态
        let snapshot: ExecutionSnapshotFile =
            read_execution_snapshot(target.join("snapshot.json")).unwrap();
        assert_eq!(snapshot.schema_version, "1.0");
        assert_eq!(snapshot.masking, MaskingMode::Secrets);
        assert_eq!(snapshot.environment.as_deref(), Some("staging"));

        // 显式 secret 值绝不落盘（三文件逐一验证）
        for name in IncidentArtifactNames::ALL {
            let content = std::fs::read(target.join(name)).unwrap();
            let content = String::from_utf8_lossy(&content);
            assert!(
                !content.contains(secret),
                "secret leaked into {name}: {content}"
            );
        }
    }

    /// 目标已存在：拒绝覆盖且既有内容原样（复用安全原语的 fail-closed 契约）
    #[test]
    fn test_create_incident_bundle_refuses_existing_target() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("incident");
        std::fs::create_dir(&target).unwrap();
        let sentinel = target.join("sentinel.txt");
        std::fs::write(&sentinel, b"keep-me").unwrap();

        let batch: Vec<(PathBuf, Vec<TestResult>)> = vec![(
            PathBuf::from("a.http"),
            vec![TestResult::error(
                1,
                None,
                "GET".into(),
                "http://x".into(),
                "e".into(),
                Duration::from_millis(1),
            )],
        )];
        let err = create_incident_bundle(&batch, &["a.http".to_string()], None, &[], 4096, &target)
            .unwrap_err();
        assert!(
            matches!(err, crate::error::RupostError::SecurityViolation(_)),
            "{err:?}"
        );
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep-me");
        for name in IncidentArtifactNames::ALL {
            assert!(!target.join(name).exists(), "no artifact may be written");
        }
    }
}
