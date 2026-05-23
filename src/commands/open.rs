use crate::cache::{format_bytes};
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
struct LocalEntry {
    label: String,
    path: PathBuf,
}

pub fn open_project(project: Option<String>, store: &ProjectStore) -> Result<()> {
    if let Ok(cwd) = env::current_dir() {
        warn_about_uncommitted_changes(&cwd, "open another project", true)?;
    }

    let selection = if let Some(p) = project {
        search_and_select_project(&p, store)?
    } else {
        let managed_projects = store.load()?;
        let local_entries = merge_local_entries(&managed_projects)?;

        if local_entries.is_empty() {
            println!("No local repositories found. Use `rem get` to clone a remote repository.");
            return Ok(());
        }

        let choices: Vec<String> = local_entries
            .iter()
            .map(|e| format!("{}  {}", e.label, e.path.display()))
            .collect();
        let selected = choose_one("Local repositories", &choices)?;
        match selected {
            Some(line) => {
                let name = parse_choice_name(&line)?;
                normalize_project_id(&name)
            }
            None => {
                println!("Cancelled.");
                return Ok(());
            }
        }
    };

    let path = open_local_repo(&selection)?;
    store.upsert_access(&selection.id, &path)?;
    println!("Opening {} at {}", selection.id, path.display());
    open_subshell(&selection.owner_repo, &path)
}

fn merge_local_entries(managed: &[Project]) -> Result<Vec<LocalEntry>> {
    let mut entries: Vec<LocalEntry> = managed
        .iter()
        .map(|p| LocalEntry {
            label: p.owner_repo().to_string(),
            path: p.path.clone(),
        })
        .collect();

    let managed_ids: std::collections::HashSet<_> =
        managed.iter().map(|p| p.id.as_str()).collect();

    for repo in list_local_repositories()? {
        if !managed_ids.contains(repo.id.as_str()) {
            entries.push(LocalEntry {
                label: repo.owner_repo,
                path: repo.path,
            });
        }
    }

    Ok(entries)
}

fn open_local_repo(selection: &RepoSelection) -> Result<PathBuf> {
    ghq_list_exact(&selection.id)?.with_context(|| {
        format!(
            "{} not found locally. Use `rem get {}` to clone it.",
            selection.owner_repo, selection.owner_repo
        )
    })
}

fn parse_choice_name(line: &str) -> Result<String> {
    line.split_whitespace()
        .next()
        .map(|s| s.to_string())
        .context("unexpected empty selection line")
}

fn search_and_select_project(
    query: &str,
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
        });
    }

    for repo in &local_repos {
        if !managed_ids.contains(repo.id.as_str()) {
            all_local.push(SearchCandidate {
                id: repo.id.clone(),
                owner_repo: repo.owner_repo.clone(),
                path: Some(repo.path.display().to_string()),
            });
        }
    }

    let matches: Vec<SearchCandidate> = all_local
        .iter()
        .filter(|c| {
            c.id.to_lowercase().contains(&query_lower)
                || c.owner_repo.to_lowercase().contains(&query_lower)
        })
        .cloned()
        .collect();

    match matches.len() {
        0 => anyhow::bail!("no local projects found matching '{}'", query),
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
                .map(|c| format!("{}  {}", c.owner_repo, c.path.as_deref().unwrap()))
                .collect();

            let selected = choose_one("Select local project", &choices)?
                .context("no project selected")?;
            let name = parse_choice_name(&selected)?;
            resolve_project(Some(name))
        }
    }
}

#[derive(Debug, Clone)]
struct SearchCandidate {
    id: String,
    owner_repo: String,
    path: Option<String>,
}