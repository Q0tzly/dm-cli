use crate::project::Project;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncommittedChanges {
    pub root: PathBuf,
    pub entries: Vec<String>,
}

pub fn uncommitted_changes(path: &Path) -> Result<Option<UncommittedChanges>> {
    let Some(root) = repo_root(path)? else {
        return Ok(None);
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("status")
        .arg("--porcelain=v1")
        .output()
        .with_context(|| format!("failed to inspect git status in {}", root.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout).context("git status output was not UTF-8")?;
    let entries = stdout
        .lines()
        .filter(|line| has_uncommitted_change(line))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(UncommittedChanges { root, entries }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpushedCommits {
    pub root: PathBuf,
    pub entries: Vec<String>,
}

pub fn unpushed_commits(path: &Path) -> Result<Option<UnpushedCommits>> {
    let Some(root) = repo_root(path)? else {
        return Ok(None);
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("log")
        .arg("--oneline")
        .arg("@{u}..HEAD")
        .output()
        .with_context(|| format!("failed to check unpushed commits in {}", root.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout).context("git log output was not UTF-8")?;
    let entries: Vec<_> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(UnpushedCommits { root, entries }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpulledCommits {
    pub root: PathBuf,
    pub entries: Vec<String>,
}

pub fn unpulled_commits(path: &Path) -> Result<Option<UnpulledCommits>> {
    let Some(root) = repo_root(path)? else {
        return Ok(None);
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("log")
        .arg("--oneline")
        .arg("HEAD..@{u}")
        .output()
        .with_context(|| format!("failed to check unpulled commits in {}", root.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout).context("git log output was not UTF-8")?;
    let entries: Vec<_> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(UnpulledCommits { root, entries }))
    }
}

pub fn pull_ff_only(path: &Path) -> Result<bool> {
    if !path.exists() {
        bail!("path does not exist: {}", path.display());
    }
    let root = repo_root(path)?.context("not a git repository")?;

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("pull")
        .arg("--ff-only")
        .output()
        .with_context(|| format!("failed to pull in {}", root.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("git pull failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(!stdout.contains("Already up to date"))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitCounts {
    pub uncommitted: usize,
    pub ahead: usize,
    pub behind: usize,
}

pub fn compute_git_counts(projects: &[Project], progress: Option<&ProgressBar>) -> Vec<GitCounts> {
    compute_git_counts_for_paths(projects.iter().map(|project| &project.path), progress)
}

pub fn compute_git_counts_for_paths<'a>(
    paths: impl Iterator<Item = &'a PathBuf>,
    progress: Option<&ProgressBar>,
) -> Vec<GitCounts> {
    let progress = progress.cloned();
    std::thread::scope(|scope| {
        paths
            .map(|path| {
                let path = path.clone();
                let progress = progress.clone();
                scope.spawn(move || {
                    let counts = GitCounts {
                        uncommitted: uncommitted_changes(&path)
                            .ok()
                            .flatten()
                            .map(|changes| changes.entries.len())
                            .unwrap_or(0),
                        ahead: unpushed_commits(&path)
                            .ok()
                            .flatten()
                            .map(|commits| commits.entries.len())
                            .unwrap_or(0),
                        behind: unpulled_commits(&path)
                            .ok()
                            .flatten()
                            .map(|commits| commits.entries.len())
                            .unwrap_or(0),
                    };
                    if let Some(progress) = progress {
                        progress.inc(1);
                    }
                    counts
                })
            })
            .map(|handle| handle.join().unwrap())
            .collect()
    })
}

pub fn repo_root(path: &Path) -> Result<Option<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .with_context(|| format!("failed to inspect git repository at {}", path.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout).context("git root output was not UTF-8")?;
    Ok(stdout.lines().next().map(PathBuf::from))
}

fn has_uncommitted_change(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() < 2 {
        return false;
    }

    bytes[0] != b' ' || bytes[1] != b' '
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_all_uncommitted_porcelain_lines() {
        assert!(has_uncommitted_change(" M src/main.rs"));
        assert!(has_uncommitted_change("?? new.txt"));
        assert!(has_uncommitted_change("AM src/main.rs"));
        assert!(has_uncommitted_change("M  src/main.rs"));
        assert!(!has_uncommitted_change(""));
    }
}
