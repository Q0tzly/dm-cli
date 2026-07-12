use crate::config::{AutomationMode, Config};
use crate::duration::parse_age;
use crate::git::repo_root;
use crate::github::project_id_from_path;
use crate::store::ProjectStore;
use anyhow::Result;
use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn touch_project(
    store: &ProjectStore,
    config: &Config,
    path: Option<PathBuf>,
    quiet: bool,
) -> Result<()> {
    if config.automation.touch == AutomationMode::Off {
        return Ok(());
    }
    let path = match path {
        Some(path) => path,
        None => env::current_dir()?,
    };
    let Some(root) = repo_root(&path)? else {
        return Ok(());
    };

    let session_id =
        env::var("REPOM_SESSION_ID").unwrap_or_else(|_| std::process::id().to_string());
    let touched = if store.touch_path_with_lease(&root, &session_id)? {
        true
    } else if let Some(project_id) = project_id_from_path(&root)? {
        store.record_usage_with_lease(&project_id, &root, &session_id)?;
        true
    } else {
        false
    };
    if touched {
        if !quiet {
            println!("Recorded recent use for {}.", root.display());
        }
        maybe_start_automatic_gc(store, config)?;
    }
    Ok(())
}

fn maybe_start_automatic_gc(store: &ProjectStore, config: &Config) -> Result<()> {
    if config.automation.clean != AutomationMode::Auto {
        return Ok(());
    }

    let interval = parse_age(&config.cleanup.check_interval)?;
    if interval.num_seconds() <= 0 {
        anyhow::bail!("cleanup.check_interval must be greater than zero");
    }
    if !store.try_begin_automatic_check(interval)? {
        return Ok(());
    }

    let executable = env::current_exe()?;
    Command::new(executable)
        .args(["gc", "--mode", "auto"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}
