use super::types::{PluginDirective, PluginStage};

/// 操作系统 Stdio IPC 管道硬限制载荷阈值 (10MB，防止打满管道缓冲区挂死)
pub const MAX_PLUGIN_PAYLOAD_BYTES: usize = 10 * 1024 * 1024;

/// 插件外部子进程默认执行超时 (5 秒硬熔断)
pub const DEFAULT_PLUGIN_TIMEOUT_SECS: u64 = 5;

/// 插件生命周期管线调度器 (Pipeline Dispatcher)
#[derive(Debug, Clone, Default)]
pub struct PluginPipeline;

impl PluginPipeline {
    /// 对声明的插件注解集合执行确定性排序仲裁：
    /// 1. 严格依据生命周期阶段不可逆流升序 (Prepare -> Transform -> Sign)；
    /// 2. 同一阶段内依据 priority 升序排序；
    /// 3. priority 相同保留书写声明保底顺序 (稳定排序)。
    pub fn sort_directives(directives: &mut [PluginDirective]) {
        directives.sort_by(|a, b| {
            a.stage
                .cmp(&b.stage)
                .then_with(|| a.priority.cmp(&b.priority))
        });
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
    fn test_sort_directives_stage_and_priority_arbitration() {
        let mut directives = vec![
            // 乱序声明：先声明了 Sign，再声明了 Prepare，再声明了 Transform
            PluginDirective {
                directive: "sign".to_string(),
                name: "aws-sigv4".to_string(),
                stage: PluginStage::Sign,
                priority: 80,
                optional: false,
                params: HashMap::new(),
            },
            PluginDirective {
                directive: "auth".to_string(),
                name: "oauth2-token".to_string(),
                stage: PluginStage::Prepare,
                priority: 20,
                optional: false,
                params: HashMap::new(),
            },
            PluginDirective {
                directive: "transform".to_string(),
                name: "aes-encrypt".to_string(),
                stage: PluginStage::Transform,
                priority: 50,
                optional: false,
                params: HashMap::new(),
            },
            PluginDirective {
                directive: "auth".to_string(),
                name: "api-key".to_string(),
                stage: PluginStage::Prepare,
                priority: 10, // 同在 Prepare，但 priority 更小，应该排在前面
                optional: true,
                params: HashMap::new(),
            },
        ];

        PluginPipeline::sort_directives(&mut directives);

        let names: Vec<&str> = directives.iter().map(|d| d.name.as_str()).collect();
        // 期望顺序：api-key (Prepare, 10) -> oauth2-token (Prepare, 20) -> aes-encrypt (Transform, 50) -> aws-sigv4 (Sign, 80)
        assert_eq!(
            names,
            vec!["api-key", "oauth2-token", "aes-encrypt", "aws-sigv4"]
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
