use crate::cache::progress_spinner;
use crate::github::{ensure_local_repo, list_remote_repositories, normalize_project_id};
use crate::select::choose_one;
use crate::shell::open_subshell;
use crate::store::ProjectStore;
use anyhow::{Context, Result};

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
        let selected =
            choose_one("Remote repositories", &choices)?.context("no repository selected")?;
        normalize_project_id(&selected)
    };

    let path = ensure_local_repo(&selection)?;
    let project = store.upsert_access(&selection.id, &path)?;
    println!("Opened {} at {}", selection.id, path.display());
    open_subshell(project.owner_repo(), &path)
}
