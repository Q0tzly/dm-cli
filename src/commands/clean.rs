use crate::cache::{clean_project, format_bytes, progress_bar, refresh_cache_sizes};
use crate::commands::util::{confirm, warn_about_uncommitted_changes};
use crate::config::Config;
use crate::duration::parse_age;
use crate::project::Project;
use crate::store::ProjectStore;
use anyhow::{Context, Result};
use chrono::Utc;

pub fn clean_projects(store: &ProjectStore, config: &Config, all: bool, yes: bool) -> Result<()> {
    let mut projects = store.load()?;
    if projects.is_empty() {
        println!("No managed projects to clean.");
        return Ok(());
    }

    refresh_cache_sizes(&mut projects, &config.cache_targets)?;

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

    refresh_cache_sizes(&mut projects, &config.cache_targets)?;
    store.save(&projects)?;
    println!("Removed {}.", format_bytes(removed_total));
    Ok(())
}
