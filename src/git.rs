use anyhow::{Context, Result};
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

fn repo_root(path: &Path) -> Result<Option<PathBuf>> {
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
