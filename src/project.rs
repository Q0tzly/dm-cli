use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub path: PathBuf,
    pub last_accessed_at: DateTime<Utc>,
    pub cache_size_bytes: Option<u64>,
}

impl Project {
    pub fn new(id: impl Into<String>, path: PathBuf) -> Self {
        Self {
            id: id.into(),
            path,
            last_accessed_at: Utc::now(),
            cache_size_bytes: None,
        }
    }

    pub fn owner_repo(&self) -> &str {
        self.id
            .strip_prefix("github.com/")
            .unwrap_or(self.id.as_str())
    }
}
