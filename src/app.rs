use crate::cache::{
    clean_project, format_bytes, progress_bar, progress_spinner, scan_project_cache_size,
};
use crate::cli::{Cli, Command};
use crate::config::{config_path, Config};
use crate::duration::{human_days_since, parse_age};
use crate::git::{pull_ff_only, uncommitted_changes, unpulled_commits, unpushed_commits};
use crate::github::{
    ensure_local_repo, list_local_repositories, list_remote_repositories, resolve_project,
    RepoSelection,
};
use crate::paths::XdgPathProvider;
use crate::project::{Project, ProjectStatus};
use crate::select::choose_one;
use crate::shell::open_subshell;
use crate::store::ProjectStore;
use anyhow::{Context, Result};
use chrono::Utc;
use clap::{CommandFactory, Parser};
use std::env;
use std::io::{self, Write};
use std::process::Command as ShellCommand;

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = XdgPathProvider;
    let config = Config::load(&paths)?;
    let store = ProjectStore::new(&paths)?;

    match cli.command {
        Some(Command::Open { project, all }) => open_project(project, all, &store),
        Some(Command::Cd { project }) => open_project(project, false, &store),
        Some(Command::List { all }) => list_projects(&store, &config, all),
        Some(Command::Close { project, yes }) => close_project(&store, &config, &project, yes),
        Some(Command::Clean { all, yes }) => clean_projects(&store, &config, all, yes),
        Some(Command::Status) => status_projects(&store, &config),
        Some(Command::Sync) => sync_projects(&store),
        Some(Command::Config { edit }) => show_config(&paths, edit),
        Some(Command::Prune { yes }) => prune_projects(&store, yes),
        Some(Command::Log) => log_projects(&store),
        Some(Command::Init { shell }) => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
            Ok(())
        }
        None => dashboard_projects(&store, &config),
    }
}

fn open_project(project: Option<String>, include_remote: bool, store: &ProjectStore) -> Result<()> {
    warn_about_uncommitted_changes(
        &env::current_dir().context("failed to determine current directory")?,
        "open another project",
        true,
    )?;

    let selection = if let Some(p) = project {
        search_and_select_project(&p, include_remote, store)?
    } else {
        let projects = store.load()?;
        if projects.is_empty() {
            resolve_project(None)?
        } else {
            let choices: Vec<String> = projects
                .iter()
                .map(|p| format!("{}  {}", p.owner_repo(), p.path.display()))
                .collect();
            let selected = choose_one("Managed projects", &choices)?;
            match selected {
                Some(line) => {
                    let name = line.split_whitespace().next().unwrap().to_string();
                    resolve_project(Some(name))?
                }
                None => resolve_project(None)?,
            }
        }
    };

    let path = ensure_local_repo(&selection)?;
    store.upsert_access(&selection.id, &path)?;
    println!("Opening {} at {}", selection.id, path.display());
    open_subshell(&selection.owner_repo, &path)
}

fn search_and_select_project(
    query: &str,
    include_remote: bool,
    store: &ProjectStore,
) -> Result<RepoSelection> {
    let query_lower = query.to_lowercase();

    let managed_projects = store.load()?;
    let local_repos = list_local_repositories()?;

    let managed_ids: std::collections::HashSet<_> =
        managed_projects.iter().map(|p| p.id.as_str()).collect();

    let mut all_local: Vec<SearchCandidate> = Vec::new();

    for p in &managed_projects {
        all_local.push(SearchCandidate {
            id: p.id.clone(),
            owner_repo: p.owner_repo().to_string(),
            path: Some(p.path.display().to_string()),
            is_managed: true,
            is_remote: false,
        });
    }

    for repo in &local_repos {
        if !managed_ids.contains(repo.id.as_str()) {
            all_local.push(SearchCandidate {
                id: repo.id.clone(),
                owner_repo: repo.owner_repo.clone(),
                path: Some(repo.path.display().to_string()),
                is_managed: false,
                is_remote: false,
            });
        }
    }

    let mut matches: Vec<SearchCandidate> = all_local
        .iter()
        .filter(|c| {
            c.id.to_lowercase().contains(&query_lower)
                || c.owner_repo.to_lowercase().contains(&query_lower)
        })
        .cloned()
        .collect();

    if include_remote {
        let spinner = progress_spinner("Searching remote repositories");
        let remote_repos = list_remote_repositories()?;
        spinner.finish_and_clear();

        let local_ids: std::collections::HashSet<_> =
            all_local.iter().map(|c| c.owner_repo.as_str()).collect();

        for repo in remote_repos {
            if repo.to_lowercase().contains(&query_lower)
                && !local_ids.contains(repo.as_str())
            {
                matches.push(SearchCandidate {
                    id: format!("github.com/{}", repo),
                    owner_repo: repo,
                    path: None,
                    is_managed: false,
                    is_remote: true,
                });
            }
        }
    }

    match matches.len() {
        0 => {
            if include_remote {
                anyhow::bail!("no projects found matching '{}' (local and remote)", query)
            } else {
                anyhow::bail!("no local projects found matching '{}'; use -a to include remote", query)
            }
        }
        1 => {
            let candidate = &matches[0];
            Ok(RepoSelection {
                id: candidate.id.clone(),
                owner_repo: candidate.owner_repo.clone(),
            })
        }
        _ => {
            let choices: Vec<String> = matches
                .iter()
                .map(|c| {
                    if let Some(path) = &c.path {
                        format!("{}  {}", c.owner_repo, path)
                    } else {
                        format!("{}  (remote)", c.owner_repo)
                    }
                })
                .collect();

            let prompt = if include_remote {
                "Select project (local + remote)"
            } else {
                "Select local project"
            };

            let selected = choose_one(prompt, &choices)?.context("no project selected")?;
            let name = selected.split_whitespace().next().unwrap().to_string();
            resolve_project(Some(name))
        }
    }
}

#[derive(Debug, Clone)]
struct SearchCandidate {
    id: String,
    owner_repo: String,
    path: Option<String>,
    is_managed: bool,
    is_remote: bool,
}

fn list_projects(store: &ProjectStore, config: &Config, include_remote: bool) -> Result<()> {
    let rows = load_dashboard_rows(store, config, include_remote)?;
    print_dashboard_rows(&rows);
    Ok(())
}

fn dashboard_projects(store: &ProjectStore, _config: &Config) -> Result<()> {
    let projects = store.load()?;
    let total = projects.len();
    let activated = projects.iter().filter(|p| p.status == ProjectStatus::Activated).count();

    println!();
    println!("          ██████╗ ███████╗██████╗  ██████╗ ███╗   ███╗");
    println!("          ██╔══██╗██╔════╝██╔══██╗██╔═══██╗████╗ ████║");
    println!("          ██████╔╝█████╗  ██████╔╝██║   ██║██╔████╔██║");
    println!("          ██╔══██╗██╔══╝  ██╔═══╝ ██║   ██║██║╚██╔╝██║");
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
        "{:<36} {:<10} {:>8} {:>6} {:>5} {:>5} {:>10} Path",
        "Project", "Status", "Age", "Dirty", "Ahead", "Behind", "Cache"
    );
    for row in rows {
        println!("{}", row.render());
    }
}

fn close_project(store: &ProjectStore, config: &Config, query: &str, yes: bool) -> Result<()> {
    let projects = store.load()?;
    let query_lower = query.to_lowercase();

    let matches: Vec<&Project> = projects
        .iter()
        .filter(|p| {
            p.id.to_lowercase().contains(&query_lower)
                || p.owner_repo().to_lowercase().contains(&query_lower)
        })
        .collect();

    let selected_project = match matches.len() {
        0 => {
            anyhow::bail!("no local projects found matching '{}'", query)
        }
        1 => matches[0].clone(),
        _ => {
            let choices: Vec<String> = matches
                .iter()
                .map(|p| format!("{}  {}", p.owner_repo(), p.path.display()))
                .collect();

            let selected = choose_one("Select project to close", &choices)?
                .context("no project selected")?;
            let name = selected.split_whitespace().next().unwrap().to_string();
            let selection = resolve_project(Some(name))?;

            projects
                .iter()
                .find(|p| p.id == selection.id)
                .context("selected project not found")?
                .clone()
        }
    };

    let mut projects = store.load()?;
    let Some(project_index) = projects
        .iter()
        .position(|p| p.id == selected_project.id)
    else {
        println!("{} is not managed yet.", selected_project.id);
        return Ok(());
    };
    let project = projects[project_index].clone();

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

fn sync_projects(store: &ProjectStore) -> Result<()> {
    let projects = store.load()?;
    let activated: Vec<_> = projects
        .iter()
        .filter(|p| p.status == ProjectStatus::Activated)
        .collect();

    if activated.is_empty() {
        println!("No activated projects to sync.");
        return Ok(());
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

fn log_projects(store: &ProjectStore) -> Result<()> {
    let mut projects = store.load()?;
    if projects.is_empty() {
        println!("No project history.");
        return Ok(());
    }

    projects.sort_by_key(|b| std::cmp::Reverse(b.last_accessed_at));

    println!(
        "{:<36} {:<10} {:>12} Path",
        "Project", "Status", "Last Accessed"
    );
    for p in &projects {
        let age = human_days_since(p.last_accessed_at);
        let status = match p.status {
            ProjectStatus::Activated => "Activated",
            ProjectStatus::Local => "Local",
        };
        println!(
            "{:<36} {:<10} {:>12} {}",
            p.owner_repo(),
            status,
            format!("{age}d ago"),
            p.path.display()
        );
    }
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

fn status_projects(store: &ProjectStore, config: &Config) -> Result<()> {
    let mut projects = store.load()?;
    if projects.is_empty() {
        println!("No managed projects.");
        return Ok(());
    }

    refresh_cache_sizes(&mut projects, config)?;
    store.save(&projects)?;

    let bar = progress_bar("Checking git status", projects.len() as u64);
    let counts = compute_git_counts(&projects);

    let mut rows: Vec<StatusRow> = Vec::new();
    for (project, c) in projects.iter().zip(counts.iter()) {
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

fn dashboard_rows(projects: &[Project], include_remote: bool) -> Result<Vec<DashboardRow>> {
    let counts = compute_git_counts(projects);
    let mut rows: Vec<DashboardRow> = Vec::new();
    for (project, c) in projects.iter().zip(counts.iter()) {
        let age = human_days_since(project.last_accessed_at);
        let dirty = if c.uncommitted > 0 {
            c.uncommitted.to_string()
        } else {
            "clean".to_string()
        };
        rows.push(DashboardRow {
            project: project.owner_repo().to_string(),
            status: match project.status {
                ProjectStatus::Activated => "Activated",
                ProjectStatus::Local => "Local",
            }
            .to_string(),
            age: format!("{age}d"),
            dirty,
            ahead: c.ahead.to_string(),
            behind: c.behind.to_string(),
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
            dirty: "-".to_string(),
            ahead: "-".to_string(),
            behind: "-".to_string(),
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
                dirty: "-".to_string(),
                ahead: "-".to_string(),
                behind: "-".to_string(),
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


