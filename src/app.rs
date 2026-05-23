use crate::cache::{
    clean_project, format_bytes, progress_bar, progress_spinner, scan_project_cache_size,
};
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

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = XdgPathProvider;
    let config = Config::load(&paths)?;
    let store = ProjectStore::new(&paths)?;

    match cli.command {
        Some(Command::Open { project }) => commands::open::open_project(project, &store),
        Some(Command::Cd { project }) => commands::open::open_project(project, &store),
        Some(Command::List { remote, size, log }) => {
            commands::list::list_projects(&store, &config, remote, size, log)
        }
        Some(Command::Close { project, all, yes }) => {
            commands::close::close_project(&store, &config, project, all, yes)
        }
        Some(Command::Clean { all, yes }) => commands::clean::clean_projects(&store, &config, all, yes),
        Some(Command::Status { all }) => commands::status::status_projects(&store, &config, all),
        Some(Command::Get { project }) => commands::get::get_project(project, &store),
        Some(Command::Sync) => commands::sync::sync_projects(&store),
        Some(Command::Config { edit }) => commands::config::show_config(&paths, edit),
        Some(Command::Prune { yes }) => commands::prune::prune_projects(&store, yes),
        Some(Command::Log) => commands::log::log_projects(&store, &config),
        Some(Command::Init { shell }) => commands::init::init_shell(shell),
        None => dashboard_projects(&store, &config),
    }
}

fn dashboard_projects(store: &ProjectStore, _config: &Config) -> Result<()> {
    let projects = store.load()?;
    let total = projects.len();
    let activated = projects.iter().filter(|p| p.status == ProjectStatus::Activated).count();

    println!();
    println!("          ██████╗ ███████╗██████╗  ██████╗ ███╗   ███╗");
    println!("          ██╔══██╗██╔════╝██╔══██╗██╔═══██╗████╗ ████║");
    println!("          ██████╔╝█████╗  ██████╔╝██║   ██║██╔████╔██║");
    println!("          ██╔══██╗██╔══╝  ██╔══╝  ██║   ██║██║╚██╔╝██║");
    println!("          ██║  ██║███████╗██║     ╚██████╔╝██║ ╚═╝ ██║");
    println!("          ╚═╝  ╚═╝╚══════╝╚═╝      ╚═════╝ ╚═╝     ╚═╝");
    println!();

    if total == 0 {
        println!("No projects yet.");
        println!();
        println!("Usage:");
        println!("  rem open <owner/repo>  Open a project");
        println!("  rem o <owner/repo>     Short alias for open");
        println!("  rem list                List all projects");
        println!("  rem help                Show all commands");
    } else {
        println!("Projects: {} total, {} activated", total, activated);
        println!();
        println!("Usage:");
        println!("  rem open                Select and open a project");
        println!("  rem list                Show detailed project list");
        println!("  rem status              Show git status of all projects");
        println!("  rem help                Show all commands");
    }

    println!();
    Ok(())
}

    let bar = progress_bar("Syncing", activated.len() as u64);

    let results: Vec<(String, Result<bool>)> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for project in &activated {
            let name = project.owner_repo().to_string();
            let path = project.path.clone();
            handles.push(s.spawn(move || (name, pull_ff_only(&path))));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut ok = 0u32;
    let mut fail = 0u32;
    for (name, result) in &results {
        match result {
            Ok(true) => {
                ok += 1;
                println!("  ↑ {name}");
            }
            Ok(false) => {
                println!("  · {name}");
            }
            Err(e) => {
                fail += 1;
                println!("  ✗ {name}: {e}");
            }
        }
        bar.inc(1);
    }
    bar.finish_and_clear();

    let parts = vec![
        Some(format!("{ok} updated")).filter(|_| ok > 0),
        Some(format!("{fail} failed")).filter(|_| fail > 0),
    ];
    let summary: Vec<_> = parts.into_iter().flatten().collect();
    if summary.is_empty() {
        println!("All up to date.");
    } else {
        println!("{}", summary.join(", "));
    }

    Ok(())
}

fn show_config(paths: &XdgPathProvider, edit: bool) -> Result<()> {
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
        println!(
            "cache_targets = {:?}",
            crate::config::default_cache_targets()
        );
        println!("older_than = \"14d\"");
    }
    Ok(())
}

fn prune_projects(store: &ProjectStore, yes: bool) -> Result<()> {
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

fn log_projects(store: &ProjectStore, config: &Config) -> Result<()> {
    list_projects(store, config, false, false, true)
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

fn status_projects(store: &ProjectStore, config: &Config, all: bool) -> Result<()> {
    let mut projects = store.load()?;

    if all {
        let local_repos = list_local_repositories()?;
        let managed_ids: std::collections::HashSet<_> =
            projects.iter().map(|p| p.id.as_str()).collect();

        let mut entries: Vec<StatusEntry> = projects
            .iter()
            .map(|p| StatusEntry {
                label: p.owner_repo().to_string(),
                status: match p.status {
                    ProjectStatus::Activated => "Activated",
                    ProjectStatus::Local => "Local",
                }
                .to_string(),
                cache: format_bytes(p.cache_size_bytes.unwrap_or(0)),
                path: p.path.clone(),
            })
            .collect();

        for repo in &local_repos {
            if !managed_ids.contains(repo.id.as_str()) {
                entries.push(StatusEntry {
                    label: repo.owner_repo.clone(),
                    status: "Local".to_string(),
                    cache: "-".to_string(),
                    path: repo.path.clone(),
                });
            }
        }

        if entries.is_empty() {
            println!("No local repositories.");
            return Ok(());
        }

        let bar = progress_bar("Checking git status", entries.len() as u64);
        let counts = compute_git_counts_for_paths(entries.iter().map(|e| &e.path));

        let mut rows: Vec<StatusRow> = Vec::new();
        for (entry, c) in entries.iter().zip(counts.iter()) {
            rows.push(StatusRow {
                project: entry.label.clone(),
                status: entry.status.clone(),
                uncommitted: c.uncommitted,
                ahead: c.ahead,
                behind: c.behind,
                cache: entry.cache.clone(),
                path: entry.path.display().to_string(),
            });
            bar.inc(1);
        }
        bar.finish_and_clear();

        println!(
            "{:<36} {:<10} {:>6} {:>5} {:>5} {:>10} Path",
            "Project", "Status", "Dirty", "Ahead", "Behind", "Cache"
        );
        for row in &rows {
            println!("{}", row.render());
        }

        return Ok(());
    }

    // Default: activated only
    if projects.is_empty() {
        println!("No managed projects.");
        return Ok(());
    }

    refresh_cache_sizes(&mut projects, config)?;
    store.save(&projects)?;

    let displayed: Vec<Project> = projects
        .into_iter()
        .filter(|p| p.status == ProjectStatus::Activated)
        .collect();

    if displayed.is_empty() {
        println!("No activated projects. Use --all to include local projects.");
        return Ok(());
    }

    let bar = progress_bar("Checking git status", displayed.len() as u64);
    let counts = compute_git_counts(&displayed);

    let mut rows: Vec<StatusRow> = Vec::new();
    for (project, c) in displayed.iter().zip(counts.iter()) {
        rows.push(StatusRow {
            project: project.owner_repo().to_string(),
            status: match project.status {
                ProjectStatus::Activated => "Activated",
                ProjectStatus::Local => "Local",
            }
            .to_string(),
            uncommitted: c.uncommitted,
            ahead: c.ahead,
            behind: c.behind,
            cache: format_bytes(project.cache_size_bytes.unwrap_or(0)),
            path: project.path.display().to_string(),
        });
        bar.inc(1);
    }
    bar.finish_and_clear();

    println!(
        "{:<36} {:<10} {:>6} {:>5} {:>5} {:>10} Path",
        "Project", "Status", "Dirty", "Ahead", "Behind", "Cache"
    );
    for row in &rows {
        println!("{}", row.render());
    }

    Ok(())
}

struct StatusEntry {
    label: String,
    status: String,
    cache: String,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct StatusRow {
    project: String,
    status: String,
    uncommitted: usize,
    ahead: usize,
    behind: usize,
    cache: String,
    path: String,
}

impl StatusRow {
    fn render(&self) -> String {
        let dirty = if self.uncommitted > 0 {
            self.uncommitted.to_string()
        } else {
            "clean".to_string()
        };
        format!(
            "{:<36} {:<10} {:>6} {:>5} {:>5} {:>10} {}",
            self.project, self.status, dirty, self.ahead, self.behind, self.cache, self.path
        )
    }
}

struct GitCounts {
    uncommitted: usize,
    ahead: usize,
    behind: usize,
}

fn compute_git_counts_for_paths<'a>(
    paths: impl Iterator<Item = &'a PathBuf>,
) -> Vec<GitCounts> {
    std::thread::scope(|s| {
        paths
            .map(|path| {
                let path = path.clone();
                s.spawn(move || GitCounts {
                    uncommitted: uncommitted_changes(&path)
                        .ok()
                        .flatten()
                        .map(|c| c.entries.len())
                        .unwrap_or(0),
                    ahead: unpushed_commits(&path)
                        .ok()
                        .flatten()
                        .map(|c| c.entries.len())
                        .unwrap_or(0),
                    behind: unpulled_commits(&path)
                        .ok()
                        .flatten()
                        .map(|c| c.entries.len())
                        .unwrap_or(0),
                })
            })
            .map(|h| h.join().unwrap())
            .collect()
    })
}

fn compute_git_counts(projects: &[Project]) -> Vec<GitCounts> {
    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for project in projects {
            let path = project.path.clone();
            handles.push(s.spawn(move || GitCounts {
                uncommitted: uncommitted_changes(&path)
                    .ok()
                    .flatten()
                    .map(|c| c.entries.len())
                    .unwrap_or(0),
                ahead: unpushed_commits(&path)
                    .ok()
                    .flatten()
                    .map(|c| c.entries.len())
                    .unwrap_or(0),
                behind: unpulled_commits(&path)
                    .ok()
                    .flatten()
                    .map(|c| c.entries.len())
                    .unwrap_or(0),
            }));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

#[derive(Debug, Clone)]
struct DashboardRow {
    project: String;
    status: String;
    age: String;
    dirty: String;
    ahead: String;
    behind: String;
    cache: String;
    path: String;
}

impl DashboardRow {
    fn render(&self) -> String {
        format!(
            "{:<36} {:<10} {:>8} {:>6} {:>5} {:>5} {:>10} {}",
            self.project, self.status, self.age, self.dirty, self.ahead, self.behind, self.cache, self.path
        )
    }
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

    match cli.command {
        Some(Command::Open { project }) => commands::open::open_project(project, &store),
        Some(Command::Cd { project }) => commands::open::open_project(project, &store),
        Some(Command::List { remote, size, log }) => {
            commands::list::list_projects(&store, &config, remote, size, log)
        }
        Some(Command::Close { project, all, yes }) => {
            commands::close::close_project(&store, &config, project, all, yes)
        }
        Some(Command::Clean { all, yes }) => commands::clean::clean_projects(&store, &config, all, yes),
        Some(Command::Status { all }) => commands::status::status_projects(&store, &config, all),
        Some(Command::Get { project }) => commands::get::get_project(project, &store),
        Some(Command::Sync) => commands::sync::sync_projects(&store),
        Some(Command::Config { edit }) => commands::config::show_config(&paths, edit),
        Some(Command::Prune { yes }) => commands::prune::prune_projects(&store, yes),
        Some(Command::Log) => commands::log::log_projects(&store, &config),
        Some(Command::Init { shell }) => commands::init::init_shell(shell),
        None => dashboard_projects(&store, &config),
    }
}

fn show_config(paths: &XdgPathProvider, edit: bool) -> Result<()> {
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
        println!(
            "cache_targets = {:?}",
            crate::config::default_cache_targets()
        );
        println!("older_than = \"14d\"");
    }
    Ok(())
}

fn prune_projects(store: &ProjectStore, yes: bool) -> Result<()> {
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

fn log_projects(store: &ProjectStore, config: &Config) -> Result<()> {
    list_projects(store, config, false, false, true)
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

fn status_projects(store: &ProjectStore, config: &Config, all: bool) -> Result<()> {
    let mut projects = store.load()?;

    if all {
        let local_repos = list_local_repositories()?;
        let managed_ids: std::collections::HashSet<_> =
            projects.iter().map(|p| p.id.as_str()).collect();

        let mut entries: Vec<StatusEntry> = projects
            .iter()
            .map(|p| StatusEntry {
                label: p.owner_repo().to_string(),
                status: match p.status {
                    ProjectStatus::Activated => "Activated",
                    ProjectStatus::Local => "Local",
                }
                .to_string(),
                cache: format_bytes(p.cache_size_bytes.unwrap_or(0)),
                path: p.path.clone(),
            })
            .collect();

        for repo in &local_repos {
            if !managed_ids.contains(repo.id.as_str()) {
                entries.push(StatusEntry {
                    label: repo.owner_repo.clone(),
                    status: "Local".to_string(),
                    cache: "-".to_string(),
                    path: repo.path.clone(),
                });
            }
        }

        if entries.is_empty() {
            println!("No local repositories.");
            return Ok(());
        }

        let bar = progress_bar("Checking git status", entries.len() as u64);
        let counts = compute_git_counts_for_paths(entries.iter().map(|e| &e.path));

        let mut rows: Vec<StatusRow> = Vec::new();
        for (entry, c) in entries.iter().zip(counts.iter()) {
            rows.push(StatusRow {
                project: entry.label.clone(),
                status: entry.status.clone(),
                uncommitted: c.uncommitted,
                ahead: c.ahead,
                behind: c.behind,
                cache: entry.cache.clone(),
                path: entry.path.display().to_string(),
            });
            bar.inc(1);
        }
        bar.finish_and_clear();

        println!(
            "{:<36} {:<10} {:>6} {:>5} {:>5} {:>10} Path",
            "Project", "Status", "Dirty", "Ahead", "Behind", "Cache"
        );
        for row in &rows {
            println!("{}", row.render());
        }

        return Ok(());
    }

    // Default: activated only
    if projects.is_empty() {
        println!("No managed projects.");
        return Ok(());
    }

    refresh_cache_sizes(&mut projects, config)?;
    store.save(&projects)?;

    let displayed: Vec<Project> = projects
        .into_iter()
        .filter(|p| p.status == ProjectStatus::Activated)
        .collect();

    if displayed.is_empty() {
        println!("No activated projects. Use --all to include local projects.");
        return Ok(());
    }

    let bar = progress_bar("Checking git status", displayed.len() as u64);
    let counts = compute_git_counts(&displayed);

    let mut rows: Vec<StatusRow> = Vec::new();
    for (project, c) in displayed.iter().zip(counts.iter()) {
        rows.push(StatusRow {
            project: project.owner_repo().to_string(),
            status: match project.status {
                ProjectStatus::Activated => "Activated",
                ProjectStatus::Local => "Local",
            }
            .to_string(),
            uncommitted: c.uncommitted,
            ahead: c.ahead,
            behind: c.behind,
            cache: format_bytes(project.cache_size_bytes.unwrap_or(0)),
            path: project.path.display().to_string(),
        });
        bar.inc(1);
    }
    bar.finish_and_clear();

    println!(
        "{:<36} {:<10} {:>6} {:>5} {:>5} {:>10} Path",
        "Project", "Status", "Dirty", "Ahead", "Behind", "Cache"
    );
    for row in &rows {
        println!("{}", row.render());
    }

    Ok(())
}

struct StatusEntry {
    label: String,
    status: String,
    cache: String,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct StatusRow {
    project: String,
    status: String,
    uncommitted: usize,
    ahead: usize,
    behind: usize,
    cache: String,
    path: String,
}

impl StatusRow {
    fn render(&self) -> String {
        let dirty = if self.uncommitted > 0 {
            self.uncommitted.to_string()
        } else {
            "clean".to_string()
        };
        format!(
            "{:<36} {:<10} {:>6} {:>5} {:>5} {:>10} {}",
            self.project, self.status, dirty, self.ahead, self.behind, self.cache, self.path
        )
    }
}

struct GitCounts {
    uncommitted: usize,
    ahead: usize,
    behind: usize,
}

fn compute_git_counts_for_paths<'a>(
    paths: impl Iterator<Item = &'a PathBuf>,
) -> Vec<GitCounts> {
    std::thread::scope(|s| {
        paths
            .map(|path| {
                let path = path.clone();
                s.spawn(move || GitCounts {
                    uncommitted: uncommitted_changes(&path)
                        .ok()
                        .flatten()
                        .map(|c| c.entries.len())
                        .unwrap_or(0),
                    ahead: unpushed_commits(&path)
                        .ok()
                        .flatten()
                        .map(|c| c.entries.len())
                        .unwrap_or(0),
                    behind: unpulled_commits(&path)
                        .ok()
                        .flatten()
                        .map(|c| c.entries.len())
                        .unwrap_or(0),
                })
            })
            .map(|h| h.join().unwrap())
            .collect()
    })
}

fn compute_git_counts(projects: &[Project]) -> Vec<GitCounts> {
    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for project in projects {
            let path = project.path.clone();
            handles.push(s.spawn(move || GitCounts {
                uncommitted: uncommitted_changes(&path)
                    .ok()
                    .flatten()
                    .map(|c| c.entries.len())
                    .unwrap_or(0),
                ahead: unpushed_commits(&path)
                    .ok()
                    .flatten()
                    .map(|c| c.entries.len())
                    .unwrap_or(0),
                behind: unpulled_commits(&path)
                    .ok()
                    .flatten()
                    .map(|c| c.entries.len())
                    .unwrap_or(0),
            }));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

#[derive(Debug, Clone)]
struct DashboardRow {
    project: String,
    status: String,
    age: String,
    dirty: String,
    ahead: String,
    behind: String,
    cache: String,
    path: String,
}

impl DashboardRow {
    fn render(&self) -> String {
        format!(
            "{:<36} {:<10} {:>8} {:>6} {:>5} {:>5} {:>10} {}",
            self.project, self.status, self.age, self.dirty, self.ahead, self.behind, self.cache, self.path
        )
    }
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


