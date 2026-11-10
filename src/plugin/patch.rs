use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 插件经由标准 Stdio IPC 管道返回的强类型增量补丁 (Immutable Patch)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PluginPatch {
    /// 增量追加或覆盖的请求头列表
    #[serde(default)]
    pub extra_headers: Vec<(String, String)>,

    /// 经变形或加签后的请求体 (若为 None 则保持原样不变)
    #[serde(default)]
    pub mutated_body: Option<String>,

    /// 动态回传给运行时上下文的局部变量
    #[serde(default)]
    pub set_variables: HashMap<String, String>,

    /// 非致命警告信息 (统一收集录入最终测试报告与数据黑匣子)
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl PluginPatch {
    /// 判断补丁是否为空 (没有任何变更操作)
    pub fn is_empty(&self) -> bool {
        self.extra_headers.is_empty()
            && self.mutated_body.is_none()
            && self.set_variables.is_empty()
            && self.warnings.is_empty()
    }

    /// 将补丁合并到另一个现有补丁中 (用于多插件链式折叠)
    pub fn merge_with(&mut self, next: PluginPatch) {
        self.extra_headers.extend(next.extra_headers);
        if next.mutated_body.is_some() {
            self.mutated_body = next.mutated_body;
        }
        self.set_variables.extend(next.set_variables);
        self.warnings.extend(next.warnings);
    }
}
