use super::types::{PluginDirective, PluginStage};

/// 操作系统 Stdio IPC 管道硬限制载荷阈值 (10MB，防止打满管道缓冲区挂死)
pub const MAX_PLUGIN_PAYLOAD_BYTES: usize = 10 * 1024 * 1024;

/// 插件外部子进程默认执行超时 (5 秒硬熔断)
pub const DEFAULT_PLUGIN_TIMEOUT_SECS: u64 = 5;

/// 插件生命周期管线调度器 (Pipeline Dispatcher)
#[derive(Debug, Clone, Default)]
pub struct PluginPipeline;

impl PluginPipeline {
    /// 对声明的插件注解集合执行确定性排序仲裁（03 §5.2 唯一权威）：
    /// 1. 严格依据生命周期阶段不可逆流升序 (Prepare -> Transform -> Sign)；
    /// 2. 同阶段内 100% 保留 `.http` 文件书写声明行序（稳定排序，所见即所发）——
    ///    `priority` 等底层魔数已彻底废除（05 ADR-D6 防复活清单）。
    pub fn sort_directives(directives: &mut [PluginDirective]) {
        directives.sort_by(|a, b| a.stage.cmp(&b.stage));
    }

    /// 提取属于指定生命周期阶段的指令切片
    pub fn filter_by_stage(
        directives: &[PluginDirective],
        stage: PluginStage,
    ) -> Vec<&PluginDirective> {
        directives.iter().filter(|d| d.stage == stage).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginPatch;
    use std::collections::HashMap;

    #[test]
    fn test_sort_directives_stage_invariance_and_declaration_fifo() {
        let mut directives = vec![
            // 乱序声明：先声明了 Sign，再声明了 Prepare，再声明了 Transform
            PluginDirective {
                directive: "sign".to_string(),
                name: "aws-sigv4".to_string(),
                stage: PluginStage::Sign,
                optional: false,
                params: HashMap::new(),
            },
            PluginDirective {
                directive: "auth".to_string(),
                name: "oauth2-token".to_string(),
                stage: PluginStage::Prepare,
                optional: false,
                params: HashMap::new(),
            },
            PluginDirective {
                directive: "transform".to_string(),
                name: "aes-encrypt".to_string(),
                stage: PluginStage::Transform,
                optional: false,
                params: HashMap::new(),
            },
            PluginDirective {
                directive: "auth".to_string(),
                name: "api-key".to_string(),
                stage: PluginStage::Prepare,
                optional: true,
                params: HashMap::new(),
            },
        ];

        PluginPipeline::sort_directives(&mut directives);

        let names: Vec<&str> = directives.iter().map(|d| d.name.as_str()).collect();
        // 阶段不可逆：Prepare -> Transform -> Sign；
        // 同阶段内保留书写行序 FIFO（oauth2-token 先于 api-key 声明）——
        // priority 魔数已废除（05 ADR-D6），声明顺序即执行顺序
        assert_eq!(
            names,
            vec!["oauth2-token", "api-key", "aes-encrypt", "aws-sigv4"]
        );
    }

    #[test]
    fn test_patch_merge_folding() {
        let mut p1 = PluginPatch {
            extra_headers: vec![("Authorization".to_string(), "Bearer token123".to_string())],
            mutated_body: None,
            set_variables: HashMap::from([("TOKEN".to_string(), "token123".to_string())]),
            warnings: vec!["Warn 1".to_string()],
        };

        let p2 = PluginPatch {
            extra_headers: vec![("X-Signature".to_string(), "sig_abc".to_string())],
            mutated_body: Some(r#"{"encrypted": true}"#.to_string()),
            set_variables: HashMap::new(),
            warnings: vec!["Warn 2".to_string()],
        };

        p1.merge_with(p2);

        assert_eq!(p1.extra_headers.len(), 2);
        assert_eq!(p1.mutated_body.as_deref(), Some(r#"{"encrypted": true}"#));
        assert_eq!(p1.set_variables.get("TOKEN").unwrap(), "token123");
        assert_eq!(p1.warnings, vec!["Warn 1", "Warn 2"]);
    }
}
