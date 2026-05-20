use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnstagedChanges {
    pub root: PathBuf,
    pub entries: Vec<String>,
}

pub fn unstaged_changes(path: &Path) -> Result<Option<UnstagedChanges>> {
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
        .filter(|line| has_unstaged_or_untracked_change(line))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(UnstagedChanges { root, entries }))
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

fn has_unstaged_or_untracked_change(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() < 2 {
        return false;
    }

    bytes[0] == b'?' || bytes[1] != b' '
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unstaged_and_untracked_porcelain_lines() {
        assert!(has_unstaged_or_untracked_change(" M src/main.rs"));
        assert!(has_unstaged_or_untracked_change("?? new.txt"));
        assert!(has_unstaged_or_untracked_change("AM src/main.rs"));
        assert!(!has_unstaged_or_untracked_change("M  src/main.rs"));
        assert!(!has_unstaged_or_untracked_change(""));
    }
}
