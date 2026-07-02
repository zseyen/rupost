use crate::Result;
use crate::history::SnapshotEntry;
use crate::http::{Client, Request};
use colored::Colorize;
use std::path::Path;
use std::time::Instant;

pub struct ReplayExecutor {
    client: Client,
    target_url: Option<String>,
    verbose: bool,
}

#[derive(Debug)]
pub struct ReplayReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
}

impl ReplayExecutor {
    pub fn new(target_url: Option<String>, verbose: bool) -> Self {
        Self {
            client: Client::new(None),
            target_url,
            verbose,
        }
    }

    /// 执行快照重放并输出对比结果
    pub async fn replay_file<P: AsRef<Path>>(&self, path: P) -> Result<ReplayReport> {
        let suite = crate::history::read_snapshot_suite(path)?;
        println!(
            "{} Loaded snapshot containing {} requests.",
            "[*]".bold().blue(),
            suite.entries.len()
        );
        if let Some(target) = &self.target_url {
            println!(
                "{} Overriding target URL base to: {}",
                "[*]".bold().blue(),
                target.green()
            );
        }

        let mut passed = 0;
        let mut failed = 0;
        let start_time = Instant::now();

        for (idx, entry) in suite.entries.iter().enumerate() {
            println!(
                "\n{} Replaying request #{} [{} {}]...",
                "[*]".bold().blue(),
                idx + 1,
                entry.request.method.bold().magenta(),
                entry.request.url
            );

            match self.replay_entry(entry).await {
                Ok(matched) => {
                    if matched {
                        passed += 1;
                    } else {
                        failed += 1;
                    }
                }
                Err(e) => {
                    println!("  {} Replay error: {}", "[✗]".bold().red(), e);
                    failed += 1;
                }
            }
        }

        let duration = start_time.elapsed();
        println!("\n{}", "─".repeat(50));
        println!(
            "{} Replay finished in {:.2?}",
            "[*]".bold().blue(),
            duration
        );
        println!(
            "  Total: {}, Passed: {}, Failed: {}",
            suite.entries.len(),
            passed.to_string().green(),
            if failed > 0 {
                failed.to_string().red()
            } else {
                failed.to_string().normal()
            }
        );

        Ok(ReplayReport {
            total: suite.entries.len(),
            passed,
            failed,
        })
    }

    async fn replay_entry(&self, entry: &SnapshotEntry) -> Result<bool> {
        // 1. 地址覆写
        let mut target_url = entry.request.url.clone();
        if let Some(target) = &self.target_url {
            if let Ok(mut parsed_url) = url::Url::parse(&entry.request.url) {
                if let Ok(parsed_target) = url::Url::parse(target) {
                    parsed_url.set_scheme(parsed_target.scheme()).ok();
                    parsed_url.set_host(parsed_target.host_str()).ok();
                    parsed_url.set_port(parsed_target.port()).ok();
                    target_url = parsed_url.to_string();
                }
            }
        }

        // 2. 还原 Request
        let mut req = Request::new(&entry.request.method, &target_url)?;
        // 还原 Headers
        for (name, val) in &entry.request.headers {
            if let Ok(val_str) = val.to_str() {
                req = req.with_header(name.as_str(), val_str);
            }
        }
        // 还原 Body
        if let Some(body_str) = &entry.request.body {
            req = req.with_body(body_str);
        }

        // 3. 执行请求
        let start = Instant::now();
        let res = self.client.execute(req).await?;
        let elapsed = start.elapsed();

        // 4. 比对状态码与基本信息
        let expected_status = entry.response.status;
        let actual_status = res.status.code();
        let status_matched = expected_status == actual_status;

        if status_matched {
            println!(
                "  {} Status matched: {} (in {:.2?})",
                "[✓]".bold().green(),
                actual_status.to_string().green(),
                elapsed
            );
        } else {
            println!(
                "  {} Status mismatch: expected {}, got {}",
                "[✗]".bold().red(),
                expected_status.to_string().green(),
                actual_status.to_string().red()
            );
        }

        if self.verbose {
            println!("  ┌─ Replayed Response Body ──────────────────────────");
            println!("  │ {}", res.body);
            println!("  └──────────────────────────────────────────────────");
        }

        Ok(status_matched)
    }
}
