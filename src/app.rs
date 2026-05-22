use crate::cache::{
    clean_project, format_bytes, progress_bar, progress_spinner, scan_project_cache_size,
};
use crate::cli::{Cli, Command};
use crate::config::Config;
use crate::duration::{human_days_since, parse_age};
use crate::git::{uncommitted_changes, unpushed_commits};
use crate::github::{
    ensure_local_repo, list_local_repositories, list_remote_repositories, resolve_project,
};
use crate::paths::XdgPathProvider;
use crate::project::{Project, ProjectStatus};
use crate::select::{choose_many, choose_one};
use crate::shell::open_subshell;
use crate::store::ProjectStore;
use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use std::env;
use std::io::{self, Write};

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = XdgPathProvider;
    let config = Config::load(&paths)?;
    let store = ProjectStore::new(&paths)?;

    match cli.command {
        Some(Command::Open { project }) => open_project(project, &store),
        Some(Command::Cd { project }) => open_project(project, &store),
        Some(Command::List { all }) => list_projects(&store, &config, all),
        Some(Command::Close { project, yes }) => close_project(&store, &config, &project, yes),
        Some(Command::Clean { older_than, yes }) => {
            clean_projects(&store, &config, older_than, yes)
        }
        None => dashboard_projects(&store, &config),
    }
}

fn open_project(project: Option<String>, store: &ProjectStore) -> Result<()> {
    warn_about_uncommitted_changes(
        &env::current_dir().context("failed to determine current directory")?,
        "open another project",
        true,
    )?;
    let selection = resolve_project(project)?;
    let path = ensure_local_repo(&selection)?;
    store.upsert_access(&selection.id, &path)?;
    println!("Opening {} at {}", selection.id, path.display());
    open_subshell(&selection.owner_repo, &path)
}

fn list_projects(store: &ProjectStore, config: &Config, include_remote: bool) -> Result<()> {
    let rows = load_dashboard_rows(store, config, include_remote)?;
    print_dashboard_rows(&rows);
    Ok(())
}

fn dashboard_projects(store: &ProjectStore, config: &Config) -> Result<()> {
    let rows = load_dashboard_rows(store, config, false)?;
    if rows.is_empty() {
        println!("No projects yet. Open one with `dm open owner/repo`.");
        return Ok(());
    }

    let choices: Vec<_> = rows.iter().map(DashboardRow::render).collect();
    let selected = choose_one("Projects", &choices)?;
    if let Some(selected) = selected {
        let project = selected
            .split_whitespace()
            .next()
            .context("selected dashboard row did not contain a project id")?;
        return open_project(Some(project.to_string()), store);
    }

    Ok(())
}

fn load_dashboard_rows(
    store: &ProjectStore,
    config: &Config,
    include_remote: bool,
) -> Result<Vec<DashboardRow>> {
    let mut projects = store.load()?;
    refresh_cache_sizes(&mut projects, config)?;
    sort_projects_for_dashboard(&mut projects);
    store.save(&projects)?;

    dashboard_rows(&projects, include_remote)
}

fn print_dashboard_rows(rows: &[DashboardRow]) {
    if rows.is_empty() {
        println!("No managed projects yet. Open one with `dm open owner/repo`.");
        return;
    }

    println!(
        "{:<36} {:<10} {:>8} {:>12} Path",
        "Project", "Status", "Age", "Cache"
    );
    for row in rows {
        println!("{}", row.render());
    }
}

fn close_project(store: &ProjectStore, config: &Config, project: &str, yes: bool) -> Result<()> {
    let selection = resolve_project(Some(project.to_string()))?;
    let mut projects = store.load()?;
    let Some(project_index) = projects
        .iter()
        .position(|project| project.id == selection.id)
    else {
        println!("{} is not managed yet.", selection.id);
        return Ok(());
    };
    let project = projects[project_index].clone();

    let size = scan_project_cache_size(&project.path, &config.cache_targets)
        .with_context(|| format!("failed to scan {}", project.path.display()))?;
    println!(
        "Will close {} and remove {} of cache.",
        project.owner_repo(),
        format_bytes(size)
    );

    if !yes && !confirm("Continue? [y/N] ")? {
        println!("Cancelled.");
        return Ok(());
    }

    warn_about_uncommitted_changes(&project.path, "close this project", !yes)?;
    warn_about_unpushed_commits(&project.path, "close this project", !yes)?;
    let removed = clean_project(&project.path, &config.cache_targets)
        .with_context(|| format!("failed to clean {}", project.id))?;
    projects[project_index].status = ProjectStatus::Local;
    projects[project_index].cache_size_bytes = Some(0);
    store.save(&projects)?;
    println!(
        "Closed {}. Removed {}.",
        project.owner_repo(),
        format_bytes(removed)
    );
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

    for project in &selected {
        warn_about_uncommitted_changes(&project.path, "clean this project", !yes)?;
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

fn sort_projects_for_dashboard(projects: &mut [Project]) {
    projects.sort_by(|left, right| {
        let left_rank = status_rank(left.status);
        let right_rank = status_rank(right.status);

        left_rank
            .cmp(&right_rank)
            .then_with(|| right.last_accessed_at.cmp(&left.last_accessed_at))
            .then_with(|| left.owner_repo().cmp(right.owner_repo()))
    });
}

fn status_rank(status: ProjectStatus) -> u8 {
    match status {
        ProjectStatus::Activated => 0,
        ProjectStatus::Local => 1,
    }
}

#[derive(Debug, Clone)]
struct DashboardRow {
    project: String,
    status: String,
    age: String,
    cache: String,
    path: String,
}

impl DashboardRow {
    fn render(&self) -> String {
        format!(
            "{:<36} {:<10} {:>8} {:>12} {}",
            self.project, self.status, self.age, self.cache, self.path
        )
    }
}

fn dashboard_rows(projects: &[Project], include_remote: bool) -> Result<Vec<DashboardRow>> {
    let mut rows = Vec::new();
    for project in projects {
        let age = human_days_since(project.last_accessed_at);
        rows.push(DashboardRow {
            project: project.owner_repo().to_string(),
            status: match project.status {
                ProjectStatus::Activated => "Activated",
                ProjectStatus::Local => "Local",
            }
            .to_string(),
            age: format!("{age}d"),
            cache: format_bytes(project.cache_size_bytes.unwrap_or(0)),
            path: project.path.display().to_string(),
        });
    }

    let managed_ids: std::collections::HashSet<_> =
        projects.iter().map(|project| project.id.as_str()).collect();
    for repo in list_local_repositories()? {
        if managed_ids.contains(repo.id.as_str()) {
            continue;
        }
        rows.push(DashboardRow {
            project: repo.owner_repo,
            status: "Local".to_string(),
            age: "-".to_string(),
            cache: "-".to_string(),
            path: repo.path.display().to_string(),
        });
    }

    if include_remote {
        let local_ids: std::collections::HashSet<_> =
            rows.iter().map(|row| row.project.clone()).collect();
        let spinner = progress_spinner("Fetching remote repositories from GitHub");
        let remote_repositories = list_remote_repositories();
        spinner.finish_and_clear();
        for repo in remote_repositories? {
            if local_ids.contains(repo.as_str()) {
                continue;
            }
            rows.push(DashboardRow {
                project: repo,
                status: "Remote".to_string(),
                age: "-".to_string(),
                cache: "-".to_string(),
                path: "-".to_string(),
            });
        }
    }

    Ok(rows)
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

fn warn_about_unpushed_commits(
    path: &std::path::Path,
    action: &str,
    require_confirmation: bool,
) -> Result<()> {
    let Some(commits) = unpushed_commits(path)? else {
        return Ok(());
    };

    println!(
        "Warning: {} has unpushed commits:",
        commits.root.display()
    );
    for entry in commits.entries.iter().take(10) {
        println!("  {entry}");
    }
    if commits.entries.len() > 10 {
        println!("  ... and {} more", commits.entries.len() - 10);
    }

    if require_confirmation && !confirm(&format!("Continue to {action}? [y/N] "))? {
        anyhow::bail!("cancelled because unpushed commits are present");
    }

    Ok(())
}
