use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

pub fn open_subshell(project_id: &str, path: &Path) -> Result<()> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let status = Command::new(&shell)
        .current_dir(path)
        .env("DM_PROJECT", project_id)
        .status()
        .with_context(|| format!("failed to start shell {shell}"))?;

    if !status.success() {
        bail!("subshell exited with status {status}");
    }

    Ok(())
}
