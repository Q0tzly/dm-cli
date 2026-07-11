use crate::git::uncommitted_changes;
use anyhow::{Context, Result};
use std::io::{self, Write};
use std::path::Path;

pub fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    io::stdout().flush().context("failed to flush stdout")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read confirmation")?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

pub fn parse_choice_name(line: &str) -> Result<String> {
    line.split_whitespace()
        .next()
        .map(ToOwned::to_owned)
        .context("unexpected empty selection line")
}

pub fn warn_about_uncommitted_changes(
    path: &Path,
    action: &str,
    require_confirmation: bool,
) -> Result<()> {
    let Some(changes) = uncommitted_changes(path)? else {
        return Ok(());
    };

    println!(
        "Warning: {} has uncommitted changes:",
        changes.root.display()
    );
    for entry in changes.entries.iter().take(10) {
        println!("  {entry}");
    }
    if changes.entries.len() > 10 {
        println!("  ... and {} more", changes.entries.len() - 10);
    }

    if require_confirmation && !confirm(&format!("Continue to {action}? [y/N] "))? {
        anyhow::bail!("cancelled because uncommitted changes are present");
    }

    Ok(())
}
