use crate::paths::PathProvider;
use crate::project::{Project, ProjectStatus};
use anyhow::{Context, Result};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectStore {
    path: PathBuf,
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
        let temp_path = self.path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::write(&temp_path, format!("{raw}\n"))
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        fs::rename(&temp_path, &self.path).with_context(|| {
            format!(
                "failed to replace {} with {}",
                self.path.display(),
                temp_path.display()
            )
        })
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

pub fn projects_path(paths: &impl PathProvider) -> Result<PathBuf> {
    Ok(paths.data_dir()?.join("projects.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::testsupport::FixedPathProvider;

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
}
