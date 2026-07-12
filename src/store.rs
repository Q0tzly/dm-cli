use crate::paths::PathProvider;
use crate::project::{Project, ProjectStatus};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectStore {
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupHistoryTarget {
    pub label: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupHistoryEntry {
    pub completed_at: DateTime<Utc>,
    pub mode: String,
    pub reason: String,
    pub reclaimed_bytes: u64,
    #[serde(default)]
    pub targets: Vec<CleanupHistoryTarget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectLease {
    pub project_id: String,
    pub session_id: String,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AutomaticCheckState {
    started_at: DateTime<Utc>,
}

impl ProjectStore {
    pub fn new(paths: &impl PathProvider) -> Result<Self> {
        Ok(Self {
            path: projects_path(paths)?,
        })
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Vec<Project>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let raw = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }

        serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", self.path.display()))
    }

    pub fn save(&self, projects: &[Project]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let raw = serde_json::to_string_pretty(projects).context("failed to serialize projects")?;
        self.write_atomic(&self.path, &raw)
    }

    pub fn upsert_access(&self, id: &str, path: &Path) -> Result<Project> {
        let mut projects = self.load()?;
        let now = Utc::now();

        if let Some(project) = projects.iter_mut().find(|project| project.id == id) {
            project.path = path.to_path_buf();
            project.last_accessed_at = now;
            project.status = ProjectStatus::Activated;
            let updated = project.clone();
            self.save(&projects)?;
            return Ok(updated);
        }

        let mut project = Project::new(id.to_string(), path.to_path_buf());
        project.last_accessed_at = now;
        projects.push(project.clone());
        projects.sort_by(|left, right| left.id.cmp(&right.id));
        self.save(&projects)?;
        Ok(project)
    }

    pub fn touch_path_with_lease(&self, path: &Path, session_id: &str) -> Result<bool> {
        let root = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let mut projects = self.load()?;
        let mut touched_project_id = None;
        let now = Utc::now();
        let mut changed = false;
        for project in &mut projects {
            let project_path =
                fs::canonicalize(&project.path).unwrap_or_else(|_| project.path.clone());
            if root == project_path || root.starts_with(&project_path) {
                if now.signed_duration_since(project.last_accessed_at) >= touch_write_interval() {
                    project.last_accessed_at = now;
                    changed = true;
                }
                touched_project_id = Some(project.id.clone());
                break;
            }
        }
        if let Some(project_id) = touched_project_id {
            if changed {
                self.save(&projects)?;
            }
            self.refresh_lease(&project_id, session_id, now)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn record_usage_with_lease(
        &self,
        id: &str,
        path: &Path,
        session_id: &str,
    ) -> Result<Project> {
        let now = Utc::now();
        let mut projects = self.load()?;
        let mut changed = false;
        let project = if let Some(project) = projects.iter_mut().find(|project| project.id == id) {
            if project.path != path {
                project.path = path.to_path_buf();
                changed = true;
            }
            if now.signed_duration_since(project.last_accessed_at) >= touch_write_interval() {
                project.last_accessed_at = now;
                changed = true;
            }
            project.clone()
        } else {
            let mut project = Project::new(id.to_string(), path.to_path_buf());
            project.status = ProjectStatus::Local;
            projects.push(project.clone());
            projects.sort_by(|left, right| left.id.cmp(&right.id));
            changed = true;
            project
        };
        if changed {
            self.save(&projects)?;
        }
        self.refresh_lease(id, session_id, now)?;
        Ok(project)
    }

    pub fn active_project_ids(&self, timeout: chrono::Duration) -> Result<HashSet<String>> {
        let cutoff = Utc::now() - timeout;
        let mut leases = self.load_leases()?;
        let before = leases.len();
        leases.retain(|lease| lease.last_seen_at >= cutoff);
        if leases.len() != before {
            self.save_leases(&leases)?;
        }
        Ok(leases.into_iter().map(|lease| lease.project_id).collect())
    }

    pub fn try_begin_automatic_check(&self, interval: chrono::Duration) -> Result<bool> {
        let state_path = self.automatic_check_path();
        let lock_path = state_path.with_extension("lock");
        let mut recovered_stale_lock = false;
        let lock = loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(lock) => break lock,
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists
                        && !recovered_stale_lock
                        && automatic_check_lock_is_stale(&lock_path) =>
                {
                    fs::remove_file(&lock_path).with_context(|| {
                        format!("failed to remove stale lock {}", lock_path.display())
                    })?;
                    recovered_stale_lock = true;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Ok(false);
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to lock {}", lock_path.display()));
                }
            }
        };

        let result = (|| {
            let now = Utc::now();
            if state_path.exists() {
                let raw = fs::read_to_string(&state_path)
                    .with_context(|| format!("failed to read {}", state_path.display()))?;
                let state: AutomaticCheckState = serde_json::from_str(&raw)
                    .with_context(|| format!("failed to parse {}", state_path.display()))?;
                if now.signed_duration_since(state.started_at) < interval {
                    return Ok(false);
                }
            }
            let raw = serde_json::to_string_pretty(&AutomaticCheckState { started_at: now })
                .context("failed to serialize automatic check state")?;
            self.write_atomic(&state_path, &raw)?;
            Ok(true)
        })();
        drop(lock);
        fs::remove_file(&lock_path)
            .with_context(|| format!("failed to unlock {}", lock_path.display()))?;
        result
    }

    pub fn load_history(&self) -> Result<Vec<CleanupHistoryEntry>> {
        let path = self.history_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn append_history(&self, entry: CleanupHistoryEntry) -> Result<()> {
        let mut history = self.load_history()?;
        history.push(entry);
        if history.len() > 100 {
            history.drain(0..history.len() - 100);
        }
        let raw = serde_json::to_string_pretty(&history).context("failed to serialize history")?;
        let path = self.history_path();
        self.write_atomic(&path, &raw)
    }

    fn history_path(&self) -> PathBuf {
        self.path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("cleanup-history.json")
    }

    fn leases_path(&self) -> PathBuf {
        self.path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("project-leases.json")
    }

    fn automatic_check_path(&self) -> PathBuf {
        self.path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("automatic-check.json")
    }

    fn load_leases(&self) -> Result<Vec<ProjectLease>> {
        let path = self.leases_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    fn save_leases(&self, leases: &[ProjectLease]) -> Result<()> {
        let raw = serde_json::to_string_pretty(leases).context("failed to serialize leases")?;
        let path = self.leases_path();
        self.write_atomic(&path, &raw)
    }

    fn refresh_lease(&self, project_id: &str, session_id: &str, now: DateTime<Utc>) -> Result<()> {
        let mut leases = self.load_leases()?;
        let mut changed = false;
        if let Some(lease) = leases
            .iter_mut()
            .find(|lease| lease.project_id == project_id && lease.session_id == session_id)
        {
            if now.signed_duration_since(lease.last_seen_at) >= touch_write_interval() {
                lease.last_seen_at = now;
                changed = true;
            }
        } else {
            leases.push(ProjectLease {
                project_id: project_id.to_string(),
                session_id: session_id.to_string(),
                last_seen_at: now,
            });
            changed = true;
        }
        if changed {
            self.save_leases(&leases)?;
        }
        Ok(())
    }

    fn write_atomic(&self, path: &Path, raw: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let temp_path = path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::write(&temp_path, format!("{raw}\n"))
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        fs::rename(&temp_path, path).with_context(|| {
            format!(
                "failed to replace {} with {}",
                path.display(),
                temp_path.display()
            )
        })
    }

    #[cfg(test)]
    pub fn set_status(&self, id: &str, status: ProjectStatus) -> Result<Option<Project>> {
        let mut projects = self.load()?;
        let updated = if let Some(project) = projects.iter_mut().find(|project| project.id == id) {
            project.status = status;
            Some(project.clone())
        } else {
            None
        };
        self.save(&projects)?;
        Ok(updated)
    }
}

fn touch_write_interval() -> chrono::Duration {
    chrono::Duration::minutes(1)
}

fn automatic_check_lock_is_stale(path: &Path) -> bool {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.elapsed().ok())
        .map(|age| age > std::time::Duration::from_secs(300))
        .unwrap_or(false)
}

pub fn projects_path(paths: &impl PathProvider) -> Result<PathBuf> {
    Ok(paths.data_dir()?.join("projects.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::testsupport::FixedPathProvider;
    use chrono::Duration;
    use std::fs;

    #[test]
    fn resolves_projects_path_under_repom_data_dir() {
        let paths = FixedPathProvider {
            data: PathBuf::from("/tmp/data/dm"),
            config: PathBuf::from("/tmp/config/dm"),
        };

        assert_eq!(
            projects_path(&paths).unwrap(),
            PathBuf::from("/tmp/data/dm/projects.json")
        );
    }

    #[test]
    fn load_missing_store_as_empty() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path().join("projects.json"));

        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn upsert_access_inserts_and_updates_existing_project() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path().join("projects.json"));

        store
            .upsert_access("github.com/acme/app", &temp.path().join("app"))
            .unwrap();
        store
            .upsert_access("github.com/acme/app", &temp.path().join("renamed"))
            .unwrap();

        let projects = store.load().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].path, temp.path().join("renamed"));
    }

    #[test]
    fn set_status_updates_existing_project() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path().join("projects.json"));

        store
            .upsert_access("github.com/acme/app", &temp.path().join("app"))
            .unwrap();
        let project = store
            .set_status("github.com/acme/app", ProjectStatus::Local)
            .unwrap()
            .unwrap();

        assert_eq!(project.status, ProjectStatus::Local);
        assert_eq!(store.load().unwrap()[0].status, ProjectStatus::Local);
    }

    #[test]
    fn records_and_reads_an_active_project_lease() {
        let temp = tempfile::tempdir().unwrap();
        let project_path = temp.path().join("app");
        fs::create_dir(&project_path).unwrap();
        let store = ProjectStore::at(temp.path().join("projects.json"));

        store
            .upsert_access("github.com/acme/app", &project_path)
            .unwrap();

        assert!(
            store
                .touch_path_with_lease(&project_path, "shell-1")
                .unwrap()
        );
        assert!(
            store
                .active_project_ids(Duration::hours(1))
                .unwrap()
                .contains("github.com/acme/app")
        );
    }

    #[test]
    fn loads_legacy_cleanup_history_without_targets() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path().join("projects.json"));
        fs::write(
            temp.path().join("cleanup-history.json"),
            r#"[{"completed_at":"2026-07-12T00:00:00Z","mode":"ask","reason":"explicit cleanup","reclaimed_bytes":42,"projects":["acme/app"]}]"#,
        )
        .unwrap();

        let history = store.load_history().unwrap();

        assert_eq!(history[0].projects, vec!["acme/app"]);
        assert!(history[0].targets.is_empty());
    }

    #[test]
    fn records_untracked_usage_as_a_local_project() {
        let temp = tempfile::tempdir().unwrap();
        let project_path = temp.path().join("app");
        fs::create_dir(&project_path).unwrap();
        let store = ProjectStore::at(temp.path().join("projects.json"));

        let project = store
            .record_usage_with_lease("github.com/acme/app", &project_path, "shell-1")
            .unwrap();

        assert_eq!(project.status, ProjectStatus::Local);
        assert!(
            store
                .active_project_ids(Duration::hours(1))
                .unwrap()
                .contains("github.com/acme/app")
        );
    }

    #[test]
    fn rate_limits_automatic_check_starts() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::at(temp.path().join("projects.json"));

        assert!(store.try_begin_automatic_check(Duration::hours(1)).unwrap());
        assert!(!store.try_begin_automatic_check(Duration::hours(1)).unwrap());
    }
}
