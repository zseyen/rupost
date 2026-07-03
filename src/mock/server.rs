use crate::Result;
use crate::mock::matcher::MockMatcher;
use std::sync::Arc;

#[allow(async_fn_in_trait)]
pub trait MockServer: Send + Sync {
    async fn start(&self, port: u16, matcher: Arc<dyn MockMatcher>) -> Result<()>;
}

pub struct DummyMockServer;

impl MockServer for DummyMockServer {
    async fn start(&self, port: u16, matcher: Arc<dyn MockMatcher>) -> Result<()> {
        let _ = port;
        let _ = matcher;
        Ok(())
    }
}

pub mod axum_adapter;
pub use axum_adapter::AxumMockServer;

use crate::history::model::SnapshotEntry;
use crate::mock::MockCompiler;
use crate::mock::matcher::{MockRouteConfig, TrieRouteMatcher};
use crate::mock::variant::{CompareOp, ConditionSource, MockVariant, VariantCondition};
use crate::runner::{DependencyResolver, DirectoryScanner, WorkflowGraph};
use colored::Colorize;
use std::collections::HashMap;
use std::fs;

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum MockFileConfig {
    Routes(Vec<MockRouteConfig>),
    Snapshots(Vec<SnapshotEntry>),
}

fn extract_path(url_str: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url_str) {
        parsed.path().to_string()
    } else {
        let path_with_query = if url_str.starts_with('/') {
            url_str
        } else if let Some(slash_pos) = url_str.find('/') {
            &url_str[slash_pos..]
        } else {
            "/"
        };
        if let Some(q_pos) = path_with_query.find('?') {
            path_with_query[..q_pos].to_string()
        } else {
            path_with_query.to_string()
        }
    }
}

/// 解析 mock 配置文件/文档，并启动 Mock 服务器
pub async fn run_server_from_file(file_path: &str, port: u16) -> Result<()> {
    println!(
        "{} Loading configuration from: {}",
        "[*]".bold().blue(),
        file_path.cyan()
    );

    let is_doc = file_path.ends_with(".md")
        || file_path.ends_with(".markdown")
        || file_path.ends_with(".http");

    let file_config = if is_doc {
        let scanned_files = DirectoryScanner::scan(&[file_path.to_string()])?;
        let sandbox_root = std::env::current_dir()?;
        let files_map = DependencyResolver::resolve_and_parse(&scanned_files, &sandbox_root)?;

        let parse_pairs: Vec<_> = files_map
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let graph = WorkflowGraph::new(&parse_pairs);
        let execution_order = graph.resolve_execution_order()?;

        let mut all_routes = Vec::new();
        for p in execution_order {
            if let Some(parsed) = files_map.get(&p) {
                let routes = MockCompiler::compile(parsed);
                all_routes.extend(routes);
            }
        }
        MockFileConfig::Routes(all_routes)
    } else {
        let content = fs::read_to_string(file_path).map_err(crate::error::RupostError::IoError)?;
        serde_json::from_str(&content).map_err(|e| {
            crate::error::RupostError::ParseError(format!(
                "Failed to parse JSON configuration: {}",
                e
            ))
        })?
    };

    let mut matcher = TrieRouteMatcher::new();
    let route_count;

    match file_config {
        MockFileConfig::Routes(routes) => {
            route_count = routes.len();
            for r in routes {
                matcher.add_route(&r.method, &r.path, r.variants);
            }
        }
        MockFileConfig::Snapshots(snapshots) => {
            let mut groups: HashMap<(String, String), Vec<MockVariant>> = HashMap::new();
            for s in snapshots {
                let path = extract_path(&s.request.url);
                let method = s.request.method.to_uppercase();

                let condition = if let Ok(parsed) = url::Url::parse(&s.request.url) {
                    let query_pairs: Vec<(String, String)> =
                        parsed.query_pairs().into_owned().collect();
                    if let Some((k, v)) = query_pairs.first() {
                        Some(VariantCondition {
                            source: ConditionSource::Query,
                            key: k.clone(),
                            operator: CompareOp::Equals,
                            expected_value: v.clone(),
                        })
                    } else {
                        None
                    }
                } else {
                    None
                };

                let mut headers = HashMap::new();
                for (name, val) in &s.response.headers {
                    if let Ok(val_str) = val.to_str() {
                        headers.insert(name.to_string(), val_str.to_string());
                    }
                }

                let variant = MockVariant {
                    condition,
                    status: s.response.status,
                    headers,
                    response_body: s.response.body,
                };
                groups.entry((method, path)).or_default().push(variant);
            }

            route_count = groups.len();
            for ((method, path), mut variants) in groups {
                variants.sort_by_key(|v| v.condition.is_some());
                variants.reverse(); // 有条件的在前，兜底的在后
                matcher.add_route(&method, &path, variants);
            }
        }
    }

    println!(
        "{} Mock engine initialized with {} routes.",
        "[✓]".bold().green(),
        route_count
    );
    println!(
        "{} Mock server starting on: {}",
        "[*]".bold().blue(),
        format!("http://localhost:{}", port).bold().green()
    );

    let server = AxumMockServer;
    server.start(port, Arc::new(matcher)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_server_invalid_file() {
        let res = run_server_from_file("nonexistent_file.json", 9999).await;
        assert!(res.is_err());
        match res.unwrap_err() {
            crate::error::RupostError::IoError(_) => {}
            other => panic!("Expected IoError, got: {:?}", other),
        }
    }
}
