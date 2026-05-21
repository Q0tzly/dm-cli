use crate::select::choose_one;
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSelection {
    pub id: String,
    pub owner_repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRepo {
    pub id: String,
    pub owner_repo: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Login {
    login: String,
}

#[derive(Debug, Deserialize)]
struct Repo {
    full_name: String,
}

pub fn resolve_project(project: Option<String>) -> Result<RepoSelection> {
    if let Some(project) = project {
        return Ok(normalize_project_id(&project));
    }

    let owners = github_owners()?;
    let owner = choose_one("Owner", &owners)?.context("no owner selected")?;
    let repos = github_repos(&owner)?;
    let repo = choose_one("Repository", &repos)?.context("no repository selected")?;
    Ok(normalize_project_id(&repo))
}

pub fn list_remote_repositories() -> Result<Vec<String>> {
    let owners = github_owners()?;
    let mut repos = Vec::new();
    let handles = owners
        .into_iter()
        .map(|owner| std::thread::spawn(move || github_repos(&owner)))
        .collect::<Vec<_>>();

    for handle in handles {
        repos.extend(
            handle
                .join()
                .map_err(|_| anyhow!("repository listing worker panicked"))??,
        );
    }
    repos.sort();
    repos.dedup();
    Ok(repos)
}

pub fn list_local_repositories() -> Result<Vec<LocalRepo>> {
    let output = Command::new("ghq")
        .arg("list")
        .arg("--full-path")
        .current_dir(ghq_command_dir()?)
        .output()
        .context("failed to run ghq list")?;

    if !output.status.success() {
        bail!(
            "ghq list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let raw = String::from_utf8(output.stdout).context("ghq output was not UTF-8")?;
    let mut repos = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| local_repo_from_path(Path::new(line)))
        .collect::<Vec<_>>();
    repos.sort_by(|left, right| left.owner_repo.cmp(&right.owner_repo));
    repos.dedup_by(|left, right| left.id == right.id);
    Ok(repos)
}

pub fn ensure_local_repo(selection: &RepoSelection) -> Result<PathBuf> {
    if let Some(path) = ghq_list_exact(&selection.id)? {
        return Ok(path);
    }

    run_status(
        Command::new("ghq")
            .arg("get")
            .arg(&selection.owner_repo)
            .current_dir(ghq_command_dir()?),
        "failed to clone repository with ghq get",
    )?;

    ghq_list_exact(&selection.id)?
        .with_context(|| format!("ghq did not report a path for {}", selection.id))
}

pub fn normalize_project_id(input: &str) -> RepoSelection {
    let trimmed = input.trim().trim_end_matches(".git");
    let owner_repo = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("git@github.com:"))
        .or_else(|| trimmed.strip_prefix("github.com/"))
        .unwrap_or(trimmed)
        .trim_matches('/');

    RepoSelection {
        id: format!("github.com/{owner_repo}"),
        owner_repo: owner_repo.to_string(),
    }
}

fn github_owners() -> Result<Vec<String>> {
    let user: Login = gh_api_json("user")?;
    let orgs: Vec<Login> = gh_api_json("user/orgs")?;
    let mut owners = vec![user.login];
    owners.extend(orgs.into_iter().map(|org| org.login));
    owners.sort();
    owners.dedup();
    Ok(owners)
}

fn github_repos(owner: &str) -> Result<Vec<String>> {
    let repos: Vec<Repo> = gh_api_json(&format!("users/{owner}/repos?per_page=100"))?;
    let mut names: Vec<_> = repos.into_iter().map(|repo| repo.full_name).collect();
    names.sort();
    Ok(names)
}

fn gh_api_json<T: for<'de> Deserialize<'de>>(endpoint: &str) -> Result<T> {
    let output = Command::new("gh")
        .arg("api")
        .arg(endpoint)
        .output()
        .with_context(|| format!("failed to run gh api {endpoint}"))?;

    if !output.status.success() {
        bail!(
            "gh api {endpoint} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    serde_json::from_slice(&output.stdout).with_context(|| format!("failed to parse {endpoint}"))
}

fn ghq_list_exact(id: &str) -> Result<Option<PathBuf>> {
    let output = Command::new("ghq")
        .arg("list")
        .arg("--full-path")
        .arg("--exact")
        .arg(id)
        .current_dir(ghq_command_dir()?)
        .output()
        .context("failed to run ghq list")?;

    if !output.status.success() {
        return Ok(None);
    }

    let path = String::from_utf8(output.stdout).context("ghq output was not UTF-8")?;
    Ok(path
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from))
}

fn run_status(command: &mut Command, context: &str) -> Result<()> {
    let status = command.status().with_context(|| context.to_string())?;
    if !status.success() {
        bail!("{context}");
    }
    Ok(())
}

fn ghq_command_dir() -> Result<PathBuf> {
    dirs::home_dir().context("could not determine home directory for ghq")
}

fn local_repo_from_path(path: &Path) -> Option<LocalRepo> {
    let parts: Vec<_> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect();
    let github_index = parts.iter().position(|part| part == "github.com")?;
    let owner = parts.get(github_index + 1)?;
    let repo = parts.get(github_index + 2)?;
    let owner_repo = format!("{owner}/{repo}");

    Some(LocalRepo {
        id: format!("github.com/{owner_repo}"),
        owner_repo,
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_common_github_project_ids() {
        assert_eq!(
            normalize_project_id("owner/repo"),
            RepoSelection {
                id: "github.com/owner/repo".to_string(),
                owner_repo: "owner/repo".to_string()
            }
        );
        assert_eq!(
            normalize_project_id("https://github.com/owner/repo.git").id,
            "github.com/owner/repo"
        );
        assert_eq!(
            normalize_project_id("git@github.com:owner/repo.git").owner_repo,
            "owner/repo"
        );
    }

    #[test]
    fn infers_local_repo_from_ghq_path() {
        let repo = local_repo_from_path(Path::new("/work/github.com/acme/app")).unwrap();

        assert_eq!(repo.id, "github.com/acme/app");
        assert_eq!(repo.owner_repo, "acme/app");
    }
}
