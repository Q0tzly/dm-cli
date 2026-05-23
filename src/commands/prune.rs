use crate::cache::{clean_project, format_bytes, progress_bar, progress_spinner, scan_project_cache_size};
use crate::cli::{Cli, Command};
use crate::config::{config_path, Config};
use crate::duration::{human_days_since, parse_age};
use crate::git::{pull_ff_only, uncommitted_changes, unpulled_commits, unpushed_commits};
use crate::github::{
    ensure_local_repo, ghq_list_exact, list_local_repositories, list_remote_repositories,
    normalize_project_id, resolve_project, RepoSelection,
};
use crate::paths::XdgPathProvider;
use crate::project::{Project, ProjectStatus};
use crate::select::choose_one;
use crate::shell::{generate_wrapper, open_subshell};
use crate::store::ProjectStore;
use anyhow::{Context, Result};
use chrono::Utc;
use clap::{CommandFactory, Parser};
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command as ShellCommand;

pub fn prune_projects(store: &ProjectStore, yes: bool) -> Result<()> {
    let mut projects = store.load()?;
    let stale: Vec<_> = projects
        .iter()
        .filter(|p| !p.path.exists())
        .cloned()
        .collect();

    if stale.is_empty() {
        println!("No stale projects to prune.");
        return Ok(());
    }

    println!("Stale projects (directory no longer exists):");
    for p in &stale {
        println!("  {}  {}", p.owner_repo(), p.path.display());
    }

    if !yes && !confirm("Remove them from the project list? [y/N] ")? {
        println!("Cancelled.");
        return Ok(());
    }

    let stale_ids: std::collections::HashSet<_> =
        stale.iter().map(|p| p.id.clone()).collect();
    projects.retain(|p| !stale_ids.contains(&p.id));
    store.save(&projects)?;

    println!("Pruned {} project(s).", stale.len());
    Ok(())
}

fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    io::stdout().flush().context("failed to flush stdout")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read confirmation")?;
    Ok(matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}