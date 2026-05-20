use crate::cache::{clean_project, format_bytes, progress_bar, scan_project_cache_size};
use crate::cli::{Cli, Command};
use crate::config::Config;
use crate::duration::{human_days_since, parse_age};
use crate::github::{ensure_local_repo, resolve_project};
use crate::paths::XdgPathProvider;
use crate::project::Project;
use crate::select::choose_many;
use crate::shell::open_subshell;
use crate::store::ProjectStore;
use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use std::io::{self, Write};

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = XdgPathProvider;
    let config = Config::load(&paths)?;
    let store = ProjectStore::new(&paths)?;

    match cli.command {
        Some(Command::Cd { project }) => open_project(project, &store),
        Some(Command::List) => list_projects(&store, &config),
        Some(Command::Clean { older_than, yes }) => {
            clean_projects(&store, &config, older_than, yes)
        }
        None => {
            if cli.project.is_some() {
                open_project(cli.project, &store)
            } else {
                list_projects(&store, &config)
            }
        }
    }
}

fn open_project(project: Option<String>, store: &ProjectStore) -> Result<()> {
    let selection = resolve_project(project)?;
    let path = ensure_local_repo(&selection)?;
    store.upsert_access(&selection.id, &path)?;
    println!("Opening {} at {}", selection.id, path.display());
    open_subshell(&selection.owner_repo, &path)
}

fn list_projects(store: &ProjectStore, config: &Config) -> Result<()> {
    let mut projects = store.load()?;
    refresh_cache_sizes(&mut projects, config)?;
    store.save(&projects)?;

    if projects.is_empty() {
        println!("No managed projects yet. Open one with `dm owner/repo`.");
        return Ok(());
    }

    println!(
        "{:<36} {:<8} {:>8} {:>12} Path",
        "Project", "Status", "Age", "Cache"
    );
    for project in projects {
        let age = human_days_since(project.last_accessed_at);
        let cache = project.cache_size_bytes.unwrap_or(0);
        println!(
            "{:<36} {:<8} {:>7}d {:>12} {}",
            project.owner_repo(),
            status_for_age(age),
            age,
            format_bytes(cache),
            project.path.display()
        );
    }

    Ok(())
}

fn clean_projects(
    store: &ProjectStore,
    config: &Config,
    older_than: Option<String>,
    yes: bool,
) -> Result<()> {
    let mut projects = store.load()?;
    if projects.is_empty() {
        println!("No managed projects to clean.");
        return Ok(());
    }

    refresh_cache_sizes(&mut projects, config)?;
    let selected = if let Some(age) = older_than {
        let age = parse_age(&age)?;
        let cutoff = Utc::now() - age;
        projects
            .iter()
            .filter(|project| project.last_accessed_at < cutoff)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        let choices: Vec<_> = projects
            .iter()
            .map(|project| {
                format!(
                    "{}\t{}\t{}",
                    project.owner_repo(),
                    format_bytes(project.cache_size_bytes.unwrap_or(0)),
                    project.path.display()
                )
            })
            .collect();
        let selected_lines = choose_many("Clean projects", &choices)?;
        projects
            .iter()
            .filter(|project| {
                selected_lines
                    .iter()
                    .any(|line| line.starts_with(project.owner_repo()))
            })
            .cloned()
            .collect::<Vec<_>>()
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

    let bar = progress_bar("Cleaning", selected.len() as u64);
    let mut removed_total = 0;
    for project in &selected {
        removed_total += clean_project(&project.path, &config.cache_targets)
            .with_context(|| format!("failed to clean {}", project.id))?;
        bar.inc(1);
    }
    bar.finish_and_clear();

    refresh_cache_sizes(&mut projects, config)?;
    store.save(&projects)?;
    println!("Removed {}.", format_bytes(removed_total));
    Ok(())
}

fn refresh_cache_sizes(projects: &mut [Project], config: &Config) -> Result<()> {
    let bar = progress_bar("Scanning cache", projects.len() as u64);
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

fn status_for_age(age_days: i64) -> &'static str {
    match age_days {
        0..=2 => "Active",
        3..=13 => "Idle",
        _ => "Stale",
    }
}

fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    io::stdout().flush().context("failed to flush stdout")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read confirmation")?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
}
