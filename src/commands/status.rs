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

#[derive(Debug, Clone)]
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

pub fn status_projects(store: &ProjectStore, config: &Config, all: bool) -> Result<()> {
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

// Reuse git counting functions from app.rs for now
// These will eventually be moved to a git utilities module
#[derive(Debug, Clone, PartialEq, Eq)]
struct UncommittedChanges {
    pub root: PathBuf,
    pub entries: Vec<String>,
}

pub fn uncommitted_changes(path: &Path) -> Result<Option<UncommittedChanges>> {
    let Some(root) = repo_root(path)? else {
        return Ok(None);
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("status")
        .arg("--porcelain=v1")
        .output()
        .with_context(|| format!("failed to inspect git status in {}", root.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout).context("git status output was not UTF-8")?;
    let entries = stdout
        .lines()
        .filter(|line| has_uncommitted_change(line))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(UncommittedChanges { root, entries }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UnpushedCommits {
    pub root: PathBuf,
    pub entries: Vec<String>,
}

pub fn unpushed_commits(path: &Path) -> Result<Option<UnpushedCommits>> {
    let Some(root) = repo_root(path)? else {
        return Ok(None);
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("log")
        .arg("--oneline")
        .arg("@{u}..HEAD")
        .output()
        .with_context(|| format!("failed to check unpushed commits in {}", root.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout).context("git log output was not UTF-8")?;
    let entries: Vec<_> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(UnpushedCommits { root, entries }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UnpulledCommits {
    pub root: PathBuf,
    pub entries: Vec<String>,
}

pub fn unpulled_commits(path: &Path) -> Result<Option<UnpulledCommits>> {
    let Some(root) = repo_root(path)? else {
        return Ok(None);
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("log")
        .arg("--oneline")
        .arg("HEAD..@{u}")
        .output()
        .with_context(|| format!("failed to check unpulled commits in {}", root.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout).context("git log output was not UTF-8")?;
    let entries: Vec<_> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(UnpulledCommits { root, entries }))
    }
}

fn repo_root(path: &Path) -> Result<Option<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .with_context(|| format!("failed to inspect git repository at {}", path.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout).context("git root output was not UTF-8")?;
    Ok(stdout.lines().next().map(PathBuf::from))
}

fn has_uncommitted_change(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() < 2 {
        return false;
    }

    bytes[0] != b' ' || bytes[1] != b' '
}

#[derive(Debug, Clone)]
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