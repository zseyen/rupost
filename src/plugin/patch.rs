use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 插件经由标准 Stdio IPC 管道返回的强类型增量补丁 (Immutable Patch)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PluginPatch {
    /// 增量追加或大小写不敏感覆盖 (Upsert) 的请求头列表
    #[serde(default)]
    pub extra_headers: Vec<(String, String)>,

    /// 需要显式移除的请求头名称列表（大小写不敏感匹配，03 §5.3；
    /// 后序插件覆盖前序兜底 Authorization 时先 remove 再 append，杜绝重复头）
    #[serde(default)]
    pub remove_headers: Vec<String>,

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

/// HTTP 头名大小写不敏感匹配（03 §5.3 Upsert 契约）
fn header_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl PluginPatch {
    /// 判断补丁是否为空 (没有任何变更操作)
    pub fn is_empty(&self) -> bool {
        self.extra_headers.is_empty()
            && self.remove_headers.is_empty()
            && self.mutated_body.is_none()
            && self.set_variables.is_empty()
            && self.warnings.is_empty()
    }

    /// 将补丁合并到另一个现有补丁中 (用于多插件链式折叠)。
    /// Header 语义：先执行 remove_headers 清理，再对 extra_headers 做
    /// 大小写不敏感 Upsert（同名覆盖，不产生重复 Authorization 头）。
    pub fn merge_with(&mut self, next: PluginPatch) {
        for removed in &next.remove_headers {
            self.extra_headers
                .retain(|(name, _)| !header_eq(name, removed));
        }
        for (name, value) in next.extra_headers {
            match self
                .extra_headers
                .iter_mut()
                .find(|(existing, _)| header_eq(existing, &name))
            {
                Some(slot) => slot.1 = value,
                None => self.extra_headers.push((name, value)),
            }
        }
        if next.mutated_body.is_some() {
            self.mutated_body = next.mutated_body;
        }
        self.set_variables.extend(next.set_variables);
        self.warnings.extend(next.warnings);
    }

    /// 将折叠后的补丁应用到解析后的请求形态（返回新 Headers/Body；
    /// 由 executor 在变量解析后、发包前调用）
    pub fn apply_to(&self, headers: &mut Vec<(String, String)>, body: &mut Option<String>) {
        for removed in &self.remove_headers {
            headers.retain(|(name, _)| !header_eq(name, removed));
        }
        for (name, value) in &self.extra_headers {
            match headers
                .iter_mut()
                .find(|(existing, _)| header_eq(existing, name))
            {
                Some(slot) => slot.1 = value.clone(),
                None => headers.push((name.clone(), value.clone())),
            }
        }
        if let Some(new_body) = &self.mutated_body {
            *body = Some(new_body.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 04 套件 5 契约：后序插件输出的 `authorization` 覆盖前序 `Authorization`
    /// 兜底头，不产生重复 Header
    #[test]
    fn test_case_insensitive_upsert_no_duplicate() {
        let mut p = PluginPatch {
            extra_headers: vec![("Authorization".to_string(), "Bearer fallback".to_string())],
            ..Default::default()
        };
        p.merge_with(PluginPatch {
            extra_headers: vec![("authorization".to_string(), "Bearer real".to_string())],
            remove_headers: vec!["X-STALE".to_string()],
            ..Default::default()
        });
        let auths: Vec<&(String, String)> = p
            .extra_headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("authorization"))
            .collect();
        assert_eq!(auths.len(), 1, "Upsert 后不得有重复 Authorization");
        assert_eq!(auths[0].1, "Bearer real");
    }

    /// apply_to 端到端：remove 优先于 upsert，body 变形生效
    #[test]
    fn test_apply_to_headers_and_body() {
        let patch = PluginPatch {
            extra_headers: vec![("X-Signature".to_string(), "sig".to_string())],
            remove_headers: vec!["authorization".to_string()],
            mutated_body: Some("{\"encrypted\":true}".to_string()),
            ..Default::default()
        };
        let mut headers = vec![
            ("Authorization".to_string(), "Bearer old".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        let mut body = Some("{\"plain\":1}".to_string());
        patch.apply_to(&mut headers, &mut body);
        assert!(
            !headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("authorization"))
        );
        assert!(headers.contains(&("X-Signature".to_string(), "sig".to_string())));
        assert_eq!(body.as_deref(), Some("{\"encrypted\":true}"));
    }
}
