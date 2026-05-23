use crate::cache::{clean_project, format_bytes, scan_project_cache_size};
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

pub fn close_project(
    store: &ProjectStore,
    config: &Config,
    query: Option<String>,
    all: bool,
    yes: bool,
) -> Result<()> {
    if all {
        return close_all_activated(store, config, yes);
    }

    let projects = store.load()?;

    let selected_project = if let Some(q) = query {
        close_select_by_query(&projects, &q)?
    } else {
        close_select_interactive(&projects)?
    };

    let Some(selected_project) = selected_project else {
        return Ok(());
    };

    execute_close(store, config, selected_project, yes)
}

fn close_all_activated(store: &ProjectStore, config: &Config, yes: bool) -> Result<()> {
    let projects = store.load()?;
    let activated: Vec<_> = projects
        .iter()
        .filter(|p| p.status == ProjectStatus::Activated)
        .cloned()
        .collect();

    if activated.is_empty() {
        println!("No activated projects to close.");
        return Ok(());
    }

    let total_cache: u64 = activated
        .iter()
        .map(|p| {
            if p.path.exists() {
                match scan_project_cache_size(&p.path, &config.cache_targets) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Warning: failed to scan {}: {e}", p.path.display());
                        0
                    }
                }
            } else {
                0
            }
        })
        .sum();

    println!(
        "Will close {} activated project(s) and remove {} of cache.",
        activated.len(),
        format_bytes(total_cache)
    );

    if !yes && !confirm("Continue? [y/N] ")? {
        println!("Cancelled.");
        return Ok(());
    }

    let mut closed = 0u32;
    let mut errors = 0u32;
    for project in &activated {
        match execute_close_inner(store, config, project) {
            Ok(removed) => {
                closed += 1;
                println!("  ✓ {} (removed {})", project.owner_repo(), format_bytes(removed));
            }
            Err(e) => {
                errors += 1;
                eprintln!("  ✗ {}: {e}", project.owner_repo());
            }
        }
    }

    let parts: Vec<_> = [
        Some(format!("{closed} closed")).filter(|_| closed > 0),
        Some(format!("{errors} failed")).filter(|_| errors > 0),
    ]
    .into_iter()
    .flatten()
    .collect();

    if parts.is_empty() {
        println!("Nothing done.");
    } else {
        println!("{}", parts.join(", "));
    }

    Ok(())
}

fn close_select_by_query<'a>(
    projects: &'a [Project],
    query: &str,
) -> Result<Option<&'a Project>> {
    let query_lower = query.to_lowercase();
    let matches: Vec<&Project> = projects
        .iter()
        .filter(|p| {
            p.id.to_lowercase().contains(&query_lower)
                || p.owner_repo().to_lowercase().contains(&query_lower)
        })
        .collect();

    match matches.len() {
        0 => anyhow::bail!("no projects found matching '{}'", query),
        1 => Ok(Some(matches[0])),
        _ => {
            let choices: Vec<String> = matches
                .iter()
                .map(|p| format!("{}  {}", p.owner_repo(), p.path.display()))
                .collect();

            let selected = choose_one("Select project to close", &choices)?
                .context("no project selected")?;
            let name = parse_choice_name(&selected)?;
            let sel_id = format!("github.com/{name}");
            Ok(projects.iter().find(|p| p.id == sel_id || p.owner_repo() == name))
        }
    }
}

fn close_select_interactive(projects: &[Project]) -> Result<Option<&Project>> {
    let activated: Vec<&Project> = projects
        .iter()
        .filter(|p| p.status == ProjectStatus::Activated)
        .collect();

    if activated.is_empty() {
        println!("No activated projects to close.");
        return Ok(None);
    }

    let choices: Vec<String> = activated
        .iter()
        .map(|p| format!("{}  {}", p.owner_repo(), p.path.display()))
        .collect();

    let selected = choose_one("Activated projects", &choices)?;
    match selected {
        Some(line) => {
            let name = line.split_whitespace().next().context("unexpected empty selection line")?.to_string();
            Ok(projects.iter().find(|p| p.owner_repo() == name))
        }
        None => {
            println!("Cancelled.");
            Ok(None)
        }
    }
}

fn execute_close(store: &ProjectStore, config: &Config, project: &Project, yes: bool) -> Result<()> {
    let size = if project.path.exists() {
        scan_project_cache_size(&project.path, &config.cache_targets)
            .with_context(|| format!("failed to scan {}", project.path.display()))?
    } else {
        println!(
            "Warning: project directory does not exist: {}",
            project.path.display()
        );
        0
    };
    println!(
        "Will close {} and remove {} of cache.",
        project.owner_repo(),
        format_bytes(size)
    );

    if !yes && !confirm("Continue? [y/N] ")? {
        println!("Cancelled.");
        return Ok(());
    }

    let removed = execute_close_inner(store, config, project)?;

    println!(
        "Closed {}. Removed {}.",
        project.owner_repo(),
        format_bytes(removed)
    );
    Ok(())
}

fn execute_close_inner(store: &ProjectStore, config: &Config, project: &Project) -> Result<u64> {
    if let Some(changes) = uncommitted_changes(&project.path)? {
        println!("Uncommitted changes in {}:", changes.root.display());
        for entry in changes.entries.iter().take(10) {
            println!("  {entry}");
        }
        if changes.entries.len() > 10 {
            println!("  ... and {} more", changes.entries.len() - 10);
        }
        anyhow::bail!("commit or stash changes before closing");
    }

    if let Some(commits) = unpushed_commits(&project.path)? {
        println!("Unpushed commits in {}:", commits.root.display());
        for entry in commits.entries.iter().take(10) {
            println!("  {entry}");
        }
        if commits.entries.len() > 10 {
            println!("  ... and {} more", commits.entries.len() - 10);
        }
        anyhow::bail!("push commits before closing");
    }

    let removed = if project.path.exists() {
        clean_project(&project.path, &config.cache_targets)
            .with_context(|| format!("failed to clean {}", project.id))?
    } else {
        0
    };

    let mut projects = store.load()?;
    if let Some(entry) = projects.iter_mut().find(|p| p.id == project.id) {
        entry.status = ProjectStatus::Local;
        entry.cache_size_bytes = Some(0);
    }
    store.save(&projects)?;

    Ok(removed)
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