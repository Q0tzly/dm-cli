use crate::cache::{format_bytes, refresh_cache_sizes};
use crate::config::Config;
use crate::github::list_local_repositories;
use crate::project::{Project, ProjectStatus};
use crate::store::ProjectStore;
use anyhow::Result;

pub fn scan_projects(store: &ProjectStore, config: &Config) -> Result<Vec<Project>> {
    let mut projects = store.load()?;
    let local_repositories = list_local_repositories()?;

    for repository in local_repositories {
        if let Some(project) = projects
            .iter_mut()
            .find(|project| project.id == repository.id)
        {
            project.path = repository.path;
        } else {
            let mut project = Project::new(repository.id, repository.path);
            project.status = ProjectStatus::Local;
            projects.push(project);
        }
    }

    refresh_cache_sizes(&mut projects, &config.cache_targets)?;
    projects.sort_by(|left, right| left.id.cmp(&right.id));
    store.save(&projects)?;
    Ok(projects)
}

pub fn scan_command(store: &ProjectStore, config: &Config) -> Result<()> {
    let projects = scan_projects(store, config)?;
    let total_cache: u64 = projects
        .iter()
        .map(|project| project.cache_size_bytes.unwrap_or(0))
        .sum();
    println!(
        "Scanned {} project(s), {} of configured cache.",
        projects.len(),
        format_bytes(total_cache)
    );
    Ok(())
}
