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

pub fn get_project(project: Option<String>, store: &ProjectStore) -> Result<()> {
    let selection = if let Some(p) = project {
        normalize_project_id(&p)
    } else {
        let spinner = progress_spinner("Fetching remote repositories");
        let repos = list_remote_repositories()?;
        spinner.finish_and_clear();

        if repos.is_empty() {
            anyhow::bail!("No remote repositories found.");
        }

        let choices: Vec<String> = repos.iter().map(|r| r.to_string()).collect();
        let selected = choose_one("Remote repositories", &choices)?
            .context("no repository selected")?;
        normalize_project_id(&selected)
    };

    let path = ensure_local_repo(&selection)?;
    let project = store.upsert_access(&selection.id, &path)?;
    println!("Opened {} at {}", selection.id, path.display());
    open_subshell(project.owner_repo(), &path)
}