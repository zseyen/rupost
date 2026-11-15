use crate::Result;
use crate::error::RupostError;
use crate::parser::ParsedFile;
use crate::runner::path::{display_path, resolve_dep_path};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

/// 依赖闭包执行计划：拓扑执行序 + 节点到其直接依赖的映射（R-1）
pub type ClosureExecutionPlan = (Vec<PathBuf>, HashMap<PathBuf, Vec<PathBuf>>);

#[derive(Debug, Clone)]
pub struct WorkflowNode {
    pub file_path: PathBuf,
    pub depends_on: Vec<PathBuf>,
}

pub struct WorkflowGraph {
    pub nodes: HashMap<PathBuf, WorkflowNode>,
}

impl WorkflowGraph {
    pub fn new(files: &[(PathBuf, ParsedFile)]) -> Self {
        let mut nodes = HashMap::new();
        for (path, parsed) in files {
            let mut resolved_deps = Vec::new();
            for dep in &parsed.dependencies {
                let canonical_dep = resolve_dep_path(path, dep);
                resolved_deps.push(canonical_dep);
            }
            let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
            nodes.insert(
                canonical_path.clone(),
                WorkflowNode {
                    file_path: canonical_path,
                    depends_on: resolved_deps,
                },
            );
        }
        Self { nodes }
    }

    /// R-1（免重填主线）：计算 roots 的祖先依赖闭包并返回拓扑执行序与依赖映射。
    /// 闭包 = roots 自身 + 沿 `depends_on` 边可达的全部祖先（传递闭包）；
    /// 子图仅保留两端均在闭包内的边。用于 `--only --with-deps`：
    /// 调试单个下游请求前自动先执行其上游（登录/取号），免去手工重跑与 token 搬运。
    pub fn closure_execution_order(&self, roots: &[PathBuf]) -> Result<ClosureExecutionPlan> {
        let mut queue: VecDeque<PathBuf> = VecDeque::new();
        for root in roots {
            let canon = root.canonicalize().unwrap_or_else(|_| root.clone());
            if self.nodes.contains_key(&canon) {
                queue.push_back(canon);
            } else {
                return Err(RupostError::Other(format!(
                    "依赖闭包根不在已解析文件集内: {}",
                    display_path(root)
                )));
            }
        }
        let mut closure: HashMap<PathBuf, WorkflowNode> = HashMap::new();
        while let Some(cur) = queue.pop_front() {
            if closure.contains_key(&cur) {
                continue;
            }
            if let Some(node) = self.nodes.get(&cur) {
                closure.insert(cur.clone(), node.clone());
                for dep in &node.depends_on {
                    if !closure.contains_key(dep) {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }
        let closure_keys: std::collections::HashSet<PathBuf> = closure.keys().cloned().collect();
        let mut sub = WorkflowGraph { nodes: closure };
        for node in sub.nodes.values_mut() {
            node.depends_on.retain(|d| closure_keys.contains(d));
        }
        let order = sub.resolve_execution_order()?;
        let dependencies = sub
            .nodes
            .iter()
            .map(|(k, v)| (k.clone(), v.depends_on.clone()))
            .collect();
        Ok((order, dependencies))
    }

    pub fn resolve_execution_order(&self) -> Result<Vec<PathBuf>> {
        let mut in_degree: HashMap<PathBuf, usize> = HashMap::new();
        let mut adj: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

        for path in self.nodes.keys() {
            in_degree.insert(path.clone(), 0);
            adj.insert(path.clone(), Vec::new());
        }

        for (path, node) in &self.nodes {
            for dep in &node.depends_on {
                if self.nodes.contains_key(dep) {
                    *in_degree.entry(path.clone()).or_insert(0) += 1;
                    adj.entry(dep.clone()).or_default().push(path.clone());
                } else {
                    return Err(RupostError::DependencyNotFound {
                        file: display_path(path),
                        missing_dep: display_path(dep),
                    });
                }
            }
        }

        let mut queue = VecDeque::new();
        let mut zero_in_degree_nodes: Vec<PathBuf> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(k, _)| k.clone())
            .collect();
        zero_in_degree_nodes.sort();
        for node in zero_in_degree_nodes {
            queue.push_back(node);
        }

        let mut order = Vec::new();

        while let Some(u) = queue.pop_front() {
            order.push(u.clone());

            if let Some(neighbors) = adj.get(&u) {
                let mut sorted_neighbors = neighbors.clone();
                sorted_neighbors.sort();
                for v in sorted_neighbors {
                    if let Some(deg) = in_degree.get_mut(&v) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(v);
                        }
                    }
                }
            }
        }

        if order.len() < self.nodes.len() {
            let mut cyclic_nodes = Vec::new();
            for (path, &deg) in &in_degree {
                if deg > 0 {
                    cyclic_nodes.push(path.to_string_lossy().to_string());
                }
            }
            cyclic_nodes.sort();
            return Err(RupostError::CyclicDependency(format!(
                "检测到循环依赖，涉及文件: {}",
                cyclic_nodes.join(", ")
            )));
        }

        Ok(order)
    }
}
