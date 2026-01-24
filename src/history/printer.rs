use super::storage::get_storage;
use crate::Result;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, Table};

pub fn list_history(limit: usize) -> Result<()> {
    let storage = get_storage();
    let entries = storage.tail(limit)?;

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
