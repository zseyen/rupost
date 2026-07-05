use super::storage::get_storage;
use crate::Result;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, Table};

pub fn list_history(limit: usize, reverse: bool) -> Result<()> {
    let storage = get_storage();
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

        table.add_row(vec![
            Cell::new(&entry.id[..8]), // Short ID
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
    let storage = get_storage();
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
