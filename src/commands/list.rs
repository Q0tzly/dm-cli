use crate::cache::{format_bytes, progress_bar, progress_spinner, refresh_cache_sizes};
use crate::config::Config;
use crate::duration::human_days_since;
use crate::git::{GitCounts, compute_git_counts};
use crate::github::{list_local_repositories, list_remote_repositories};
use crate::project::{Project, ProjectStatus};
use crate::store::ProjectStore;
use anyhow::Result;

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
            self.project,
            self.status,
            self.age,
            self.dirty,
            self.ahead,
            self.behind,
            self.cache,
            self.path
        )
    }
}

pub fn list_projects(
    store: &ProjectStore,
    config: &Config,
    remote: bool,
    size: bool,
    log: bool,
) -> Result<()> {
    let mut projects = store.load()?;

    if log {
        projects.sort_by_key(|project| std::cmp::Reverse(project.last_accessed_at));
    }

    if size {
        refresh_cache_sizes(&mut projects, &config.cache_targets)?;
        store.save(&projects)?;
    }

    let rows = list_dashboard_rows(&projects, remote, size)?;
    list_print(&rows, size);
    Ok(())
}

fn list_dashboard_rows(
    projects: &[Project],
    remote: bool,
    size: bool,
) -> Result<Vec<DashboardRow>> {
    let counts = if size {
        let bar = progress_bar("Checking git status", projects.len() as u64);
        let counts = compute_git_counts(projects, Some(&bar));
        bar.finish_and_clear();
        counts
    } else {
        projects.iter().map(|_| GitCounts::default()).collect()
    };

    let mut rows = Vec::with_capacity(projects.len());
    for (project, counts) in projects.iter().zip(counts.iter()) {
        let age = human_days_since(project.last_accessed_at);
        rows.push(DashboardRow {
            project: project.owner_repo().to_string(),
            status: match project.status {
                ProjectStatus::Activated => "Activated",
                ProjectStatus::Local => "Local",
            }
            .to_string(),
            age: if size {
                format!("{age}d")
            } else {
                "-".to_string()
            },
            dirty: if size && counts.uncommitted > 0 {
                counts.uncommitted.to_string()
            } else {
                "-".to_string()
            },
            ahead: if size {
                counts.ahead.to_string()
            } else {
                "-".to_string()
            },
            behind: if size {
                counts.behind.to_string()
            } else {
                "-".to_string()
            },
            cache: if size {
                format_bytes(project.cache_size_bytes.unwrap_or(0))
            } else {
                "-".to_string()
            },
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

    if remote {
        let local_ids: std::collections::HashSet<_> =
            rows.iter().map(|row| row.project.clone()).collect();
        let spinner = progress_spinner("Fetching remote repositories from GitHub");
        let remote_repos = list_remote_repositories()?;
        spinner.finish_and_clear();
        for repo in remote_repos {
            if local_ids.contains(&repo) {
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

fn list_print(rows: &[DashboardRow], size: bool) {
    if rows.is_empty() {
        println!("No projects.");
        return;
    }

    if size {
        println!(
            "{:<36} {:<10} {:>8} {:>6} {:>5} {:>5} {:>10} Path",
            "Project", "Status", "Age", "Dirty", "Ahead", "Behind", "Cache"
        );
        for row in rows {
            println!("{}", row.render());
        }
    } else {
        println!("{:<36} {:<10}  Path", "Project", "Status");
        for row in rows {
            println!("{:<36} {:<10}  {}", row.project, row.status, row.path);
        }
    }
}
