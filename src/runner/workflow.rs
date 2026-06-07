use crate::Result;
use crate::error::RupostError;
use crate::parser::ParsedFile;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

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
            let parent_dir = path.parent();
            for dep in &parsed.dependencies {
                let dep_path = if let Some(parent) = parent_dir {
                    parent.join(dep)
                } else {
                    PathBuf::from(dep)
                };
                let canonical_dep = dep_path.canonicalize().unwrap_or(dep_path);
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
                        file: path.to_string_lossy().to_string(),
                        missing_dep: dep.to_string_lossy().to_string(),
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
