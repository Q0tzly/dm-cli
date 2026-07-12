use crate::config::Config;
use crate::config::config_path;
use crate::paths::XdgPathProvider;
use anyhow::{Context, Result};
use std::process::Command as ShellCommand;

pub fn show_config(paths: &XdgPathProvider, edit: bool) -> Result<()> {
    let path = config_path(paths)?;
    if edit {
        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| "vim".to_string());
        let status = ShellCommand::new(&editor)
            .arg(&path)
            .status()
            .with_context(|| format!("failed to launch {editor}"))?;
        if !status.success() {
            anyhow::bail!("{editor} exited with status {status}");
        }
        return Ok(());
    }

    println!("Config: {}", path.display());
    if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        print!("{raw}");
    } else {
        println!("(default configuration)");
        print!(
            "{}",
            toml::to_string_pretty(&Config::default())
                .context("failed to serialize default configuration")?
        );
    }
    Ok(())
}
