use crate::cache::format_bytes;
use crate::store::ProjectStore;
use anyhow::Result;

pub fn show_history(store: &ProjectStore) -> Result<()> {
    let history = store.load_history()?;
    if history.is_empty() {
        println!("No cleanup history.");
        return Ok(());
    }

    for entry in history.iter().rev().take(20) {
        println!(
            "{}  {} reclaimed  {}  {}",
            entry.completed_at.format("%Y-%m-%d %H:%M"),
            entry.mode,
            format_bytes(entry.reclaimed_bytes),
            entry.reason
        );
        if entry.targets.is_empty() {
            for project in &entry.projects {
                println!("  - {project}");
            }
        } else {
            for target in &entry.targets {
                println!("  - {:<40} {}", target.label, format_bytes(target.bytes));
            }
        }
    }
    Ok(())
}
