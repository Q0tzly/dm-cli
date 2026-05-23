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

pub fn clean_projects(
    store: &ProjectStore,
    config: &Config,
    all: bool,
    yes: bool,
) -> Result<()> {
    let mut projects = store.load()?;
    if projects.is_empty() {
        println!("No managed projects to clean.");
        return Ok(());
    }

    refresh_cache_sizes(&mut projects, config)?;

    let selected: Vec<Project> = if all {
        projects.clone()
    } else {
        let age = parse_age(&config.older_than)?;
        let cutoff = Utc::now() - age;
        projects
            .iter()
            .filter(|project| project.last_accessed_at < cutoff)
            .cloned()
            .collect()
    };

    if selected.is_empty() {
        println!("No projects selected for cleaning.");
        store.save(&projects)?;
        return Ok(());
    }

    let total: u64 = selected
        .iter()
        .map(|project| project.cache_size_bytes.unwrap_or(0))
        .sum();
    println!(
        "Will clean {} project(s), estimated cache {}.",
        selected.len(),
        format_bytes(total)
    );

    if !yes && !confirm("Continue? [y/N] ")? {
        println!("Cancelled.");
        store.save(&projects)?;
        return Ok(());
    }

    for project in &selected {
        warn_about_uncommitted_changes(&project.path, "clean this project", !yes)?;
    }

    let bar = progress_bar("Cleaning", selected.len() as u64);
    let mut removed_total = 0;
    for project in &selected {
        if project.path.exists() {
            removed_total += clean_project(&project.path, &config.cache_targets)
                .with_context(|| format!("failed to clean {}", project.id))?;
        }
        bar.inc(1);
    }
    bar.finish_and_clear();

    refresh_cache_sizes(&mut projects, config)?;
    store.save(&projects)?;
    println!("Removed {}.", format_bytes(removed_total));
    Ok(())
}

fn refresh_cache_sizes(projects: &mut [Project], config: &Config) -> Result<()> {
    let bar = progress_bar("Scanning local cache", projects.len() as u64);
    for project in projects {
        project.cache_size_bytes = if project.path.exists() {
            Some(
                scan_project_cache_size(&project.path, &config.cache_targets)
                    .with_context(|| format!("failed to scan {}", project.path.display()))?,
            )
        } else {
            Some(0)
        };
        bar.inc(1);
    }
    bar.finish_and_clear();
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

fn warn_about_uncommitted_changes(
    path: &std::path::Path,
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