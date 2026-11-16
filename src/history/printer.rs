use super::gc;
use super::storage::HistoryStorage;
use crate::Result;
use crate::platform;
use crate::utils::text::truncate_chars;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, Table};

pub fn list_history(limit: usize, reverse: bool) -> Result<()> {
    let storage = HistoryStorage::new();
    let mut entries = storage.tail(limit)?;

    if reverse {
        entries.reverse();
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec!["ID", "Time", "Method", "URL", "Status", "Duration"]);

    for entry in entries {
        let status_color = if entry.response.status < 400 {
            Color::Green
        } else {
            Color::Red
        };

        let short_id = truncate_chars(&entry.id, 8, "");
        table.add_row(vec![
            Cell::new(&short_id), // Short ID (边界安全截断)
            Cell::new(entry.timestamp.format("%H:%M:%S")),
            Cell::new(&entry.request.method),
            Cell::new(&entry.request.url).add_attribute(Attribute::Dim),
            Cell::new(entry.response.status).fg(status_color),
            Cell::new(format!("{}ms", entry.duration_ms)),
        ]);
    }

    println!("{}", table);

    Ok(())
}

pub fn show_history(target: &str) -> Result<()> {
    let storage = HistoryStorage::new();
    let entries = storage.list()?;
    if entries.is_empty() {
        println!("No history entries found.");
        return Ok(());
    }

    // 尝试解析为正整数 N，表示匹配最近最新（1-indexed）的第 N 条
    let selected_entry = if let Ok(n) = target.parse::<usize>() {
        if n == 0 || n > entries.len() {
            println!(
                "Index {} out of range. Total entries: {}.",
                n,
                entries.len()
            );
            return Ok(());
        }
        // 倒数第 n 条，即 entries.len() - n
        Some(&entries[entries.len() - n])
    } else {
        // 短 ID 匹配
        entries
            .iter()
            .find(|e| e.id.starts_with(target) || e.id == target)
    };

    if let Some(entry) = selected_entry {
        use colored::Colorize;
        println!("=====================================================");
        println!("            RuPost History Entry Details             ");
        println!("=====================================================");
        println!("{:<12}: {}", "ID", entry.id);
        println!(
            "{:<12}: {}",
            "Timestamp",
            entry.timestamp.format("%Y-%m-%d %H:%M:%S")
        );
        println!("{:<12}: {}", "Method", entry.request.method);
        println!("{:<12}: {}", "URL", entry.request.url);
        println!("{:<12}: {}", "Status", entry.response.status);
        println!("{:<12}: {}ms", "Duration", entry.duration_ms);
        println!("-----------------------------------------------------");

        if let Some(ref body) = entry.response.body {
            if body.contains("[→]") || body.contains("[←]") {
                println!("{}", " [WebSocket Session Frames Log] ".bold().yellow());
                println!("-----------------------------------------------------");
                for line in body.lines() {
                    if line.starts_with("[→]") {
                        println!("{}", line.cyan());
                    } else if line.starts_with("[←]") {
                        println!("{}", line.yellow());
                    } else {
                        println!("{}", line);
                    }
                }
            } else if body.trim().starts_with("data:") || body.contains("event:") {
                println!(
                    "{}",
                    " [Server-Sent Events (SSE) Stream Log] ".bold().yellow()
                );
                println!("-----------------------------------------------------");
                println!("{}", body.green());
            } else {
                println!("{}", " [Response Body] ".bold().yellow());
                println!("-----------------------------------------------------");
                println!("{}", body);
            }
        } else {
            println!("No body or frames recorded.");
        }
        println!("=====================================================");
    } else {
        println!("No matching history entry found for '{}'.", target);
    }

    Ok(())
}

pub fn prune_history(days: u64, max_size: usize) -> Result<()> {
    use colored::Colorize;
    let log_dir = platform::history_logs_dir();

    println!(
        "Scanning log directory: {}",
        log_dir.display().to_string().cyan()
    );
    match gc::perform_prune(&log_dir, days, max_size) {
        Ok((count, bytes)) => {
            let mb = bytes as f64 / 1024.0 / 1024.0;
            println!(
                "{} Removed {} files (Released {:.2} MB).",
                "[✓]".green().bold(),
                count,
                mb
            );
        }
        Err(e) => {
            println!("{} Failed to prune logs: {}", "[✗]".red().bold(), e);
        }
    }
    Ok(())
}

pub fn clear_history(yes: bool, all: bool) -> Result<()> {
    use colored::Colorize;
    use std::io::{Write, stdin, stdout};

    if !yes {
        print!(
            "{} {} Are you sure you want to permanently clear all logs? (y/N): ",
            "[!]".yellow().bold(),
            "WARNING: Permanent deletion ahead!".bold()
        );
        let _ = stdout().flush();
        let mut input = String::new();
        if stdin().read_line(&mut input).is_err() {
            println!("Aborted.");
            return Ok(());
        }
        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }

    let log_dir = platform::history_logs_dir();

    // 1. 清空 logs/ 文件夹
    match gc::perform_clear(&log_dir) {
        Ok((count, bytes)) => {
            let mb = bytes as f64 / 1024.0 / 1024.0;
            println!(
                "{} Cleared logs/ directory. Removed {} files (Released {:.2} MB).",
                "[✓]".green().bold(),
                count,
                mb
            );
        }
        Err(e) => {
            println!(
                "{} Failed to clear logs directory: {}",
                "[✗]".red().bold(),
                e
            );
        }
    }

    // 2. 如果携带了 --all，清空 history.jsonl
    if all {
        let history_db = platform::history_dir().join("history.jsonl");
        if history_db.exists() {
            if std::fs::remove_file(&history_db).is_ok() {
                println!("{} Cleared history.jsonl database.", "[✓]".green().bold());
            } else {
                println!(
                    "{} Failed to clear history.jsonl database.",
                    "[✗]".red().bold()
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_history_short_id_safety() {
        let short_id = "123";
        let formatted = truncate_chars(short_id, 8, "");
        assert_eq!(formatted, "123");

        let long_id = "1234567890abcdef";
        let formatted_long = truncate_chars(long_id, 8, "");
        assert_eq!(formatted_long, "12345678");
    }
}
