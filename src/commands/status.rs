use crate::cache::{format_bytes, progress_bar, refresh_cache_sizes};
use crate::config::Config;
use crate::git::{compute_git_counts, compute_git_counts_for_paths};
use crate::github::list_local_repositories;
use crate::project::{Project, ProjectStatus};
use crate::store::ProjectStore;
use anyhow::Result;
use std::path::PathBuf;

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
    refresh_cache_sizes(&mut projects, &config.cache_targets)?;
    store.save(&projects)?;

    if all {
        let local_repos = list_local_repositories()?;
        let managed_ids: std::collections::HashSet<_> =
            projects.iter().map(|project| project.id.as_str()).collect();

        let mut entries: Vec<StatusEntry> = projects
            .iter()
            .map(|project| StatusEntry {
                label: project.owner_repo().to_string(),
                status: project_status(project.status),
                cache: format_bytes(project.cache_size_bytes.unwrap_or(0)),
                path: project.path.clone(),
            })
            .collect();

        for repo in local_repos {
            if !managed_ids.contains(repo.id.as_str()) {
                entries.push(StatusEntry {
                    label: repo.owner_repo,
                    status: "Local".to_string(),
                    cache: "-".to_string(),
                    path: repo.path,
                });
            }
        }

        if entries.is_empty() {
            println!("No local repositories.");
            return Ok(());
        }

        print_status_rows(entries, "Checking git status");
        return Ok(());
    }

    if projects.is_empty() {
        println!("No managed projects.");
        return Ok(());
    }

    let displayed: Vec<Project> = projects
        .into_iter()
        .filter(|project| project.status == ProjectStatus::Activated)
        .collect();

    if displayed.is_empty() {
        println!("No activated projects. Use --all to include local projects.");
        return Ok(());
    }

    let bar = progress_bar("Checking git status", displayed.len() as u64);
    let counts = compute_git_counts(&displayed, Some(&bar));
    bar.finish_and_clear();
    let rows = displayed
        .iter()
        .zip(counts)
        .map(|(project, counts)| StatusRow {
            project: project.owner_repo().to_string(),
            status: project_status(project.status),
            uncommitted: counts.uncommitted,
            ahead: counts.ahead,
            behind: counts.behind,
            cache: format_bytes(project.cache_size_bytes.unwrap_or(0)),
            path: project.path.display().to_string(),
        })
        .collect::<Vec<_>>();
    print_rows(&rows);

    Ok(())
}

fn print_status_rows(entries: Vec<StatusEntry>, message: &str) {
    let bar = progress_bar(message, entries.len() as u64);
    let counts = compute_git_counts_for_paths(entries.iter().map(|entry| &entry.path), Some(&bar));
    bar.finish_and_clear();

    let rows = entries
        .iter()
        .zip(counts)
        .map(|(entry, counts)| StatusRow {
            project: entry.label.clone(),
            status: entry.status.clone(),
            uncommitted: counts.uncommitted,
            ahead: counts.ahead,
            behind: counts.behind,
            cache: entry.cache.clone(),
            path: entry.path.display().to_string(),
        })
        .collect::<Vec<_>>();
    print_rows(&rows);
}

fn print_rows(rows: &[StatusRow]) {
    println!(
        "{:<36} {:<10} {:>6} {:>5} {:>5} {:>10} Path",
        "Project", "Status", "Dirty", "Ahead", "Behind", "Cache"
    );
    for row in rows {
        println!("{}", row.render());
    }
}

fn project_status(status: ProjectStatus) -> String {
    match status {
        ProjectStatus::Activated => "Activated",
        ProjectStatus::Local => "Local",
    }
    .to_string()
}
