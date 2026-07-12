use crate::cache::{
    format_bytes, parse_bytes, progress_bar, refresh_cache_sizes, scan_cache_target_size,
};
use crate::config::Config;
use crate::duration::parse_age;
use crate::git::{compute_git_counts, compute_git_counts_for_paths};
use crate::github::list_local_repositories;
use crate::project::{Project, ProjectStatus};
use crate::store::ProjectStore;
use anyhow::Result;
use chrono::Utc;
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
    if !projects.is_empty() {
        print_cache_summary(&projects, config, store)?;
    }

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

fn print_cache_summary(projects: &[Project], config: &Config, store: &ProjectStore) -> Result<()> {
    let total = projects
        .iter()
        .map(|project| project.cache_size_bytes.unwrap_or(0))
        .sum::<u64>();
    let protected = projects
        .iter()
        .filter(|project| {
            project.protected
                || config
                    .cleanup
                    .protected
                    .iter()
                    .any(|value| value == &project.id || value == project.owner_repo())
        })
        .map(|project| project.cache_size_bytes.unwrap_or(0))
        .sum::<u64>();

    if let Some(maximum) = config
        .cleanup
        .max_cache_size
        .as_deref()
        .map(parse_bytes)
        .transpose()?
    {
        println!(
            "Workspace cache: {} / {} budget",
            format_bytes(total),
            format_bytes(maximum)
        );
    } else {
        println!("Workspace cache: {}", format_bytes(total));
    }
    println!("Protected cache: {}", format_bytes(protected));
    let (reclaimable, target_count) = reclaimable_cache_summary(projects, config, store)?;
    println!(
        "Reclaimable cache: {} across {} target(s)",
        format_bytes(reclaimable),
        target_count
    );
    if let Some(last_cleanup) = store.load_history()?.last() {
        println!(
            "Last cleanup: {} ({}, {})",
            last_cleanup.completed_at.format("%Y-%m-%d %H:%M"),
            format_bytes(last_cleanup.reclaimed_bytes),
            last_cleanup.reason
        );
    }
    Ok(())
}

fn reclaimable_cache_summary(
    projects: &[Project],
    config: &Config,
    store: &ProjectStore,
) -> Result<(u64, usize)> {
    let cutoff = Utc::now() - parse_age(&config.cleanup.minimum_inactive)?;
    let active_project_ids =
        store.active_project_ids(parse_age(&config.cleanup.active_lease_timeout)?)?;
    let mut reclaimable_bytes = 0;
    let mut target_count = 0;

    for project in projects {
        if project.protected
            || config
                .cleanup
                .protected
                .iter()
                .any(|value| value == &project.id || value == project.owner_repo())
            || active_project_ids.contains(&project.id)
            || !project.path.exists()
            || project.last_accessed_at >= cutoff
        {
            continue;
        }

        for target in &config.cache_targets {
            let bytes = scan_cache_target_size(&project.path, target)?;
            if bytes > 0 {
                reclaimable_bytes += bytes;
                target_count += 1;
            }
        }
    }

    Ok((reclaimable_bytes, target_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use std::fs;

    #[test]
    fn summarizes_only_inactive_unprotected_targets() {
        let temp = tempfile::tempdir().unwrap();
        let old_path = temp.path().join("old");
        fs::create_dir_all(old_path.join("target")).unwrap();
        fs::write(old_path.join("target/artifact"), [0; 10]).unwrap();
        fs::create_dir_all(old_path.join("node_modules")).unwrap();
        fs::write(old_path.join("node_modules/package"), [0; 20]).unwrap();

        let recent_path = temp.path().join("recent");
        fs::create_dir_all(recent_path.join("target")).unwrap();
        fs::write(recent_path.join("target/artifact"), [0; 30]).unwrap();

        let protected_path = temp.path().join("protected");
        fs::create_dir_all(protected_path.join("target")).unwrap();
        fs::write(protected_path.join("target/artifact"), [0; 40]).unwrap();

        let mut old = Project::new("old/repo", old_path);
        old.last_accessed_at = Utc::now() - Duration::days(2);
        let mut recent = Project::new("recent/repo", recent_path);
        recent.last_accessed_at = Utc::now();
        let mut protected = Project::new("protected/repo", protected_path);
        protected.protected = true;
        protected.last_accessed_at = Utc::now() - Duration::days(2);
        let projects = vec![old, recent, protected];

        let mut config = Config::default();
        config.cleanup.minimum_inactive = "1d".to_string();
        let store = ProjectStore::at(temp.path().join("projects.json"));

        assert_eq!(
            reclaimable_cache_summary(&projects, &config, &store).unwrap(),
            (30, 2)
        );
    }
}
