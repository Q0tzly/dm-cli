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

pub fn sync_projects(store: &ProjectStore) -> Result<()> {
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