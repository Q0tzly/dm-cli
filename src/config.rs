use crate::paths::PathProvider;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default = "default_cache_targets")]
    pub cache_targets: Vec<String>,
    #[serde(default = "default_older_than")]
    pub older_than: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cache_targets: default_cache_targets(),
            older_than: default_older_than(),
        }
    }
}

pub fn default_cache_targets() -> Vec<String> {
    vec!["target".to_string(), "node_modules".to_string()]
}

fn default_older_than() -> String {
    "14d".to_string()
}

impl Config {
    pub fn load(paths: &impl PathProvider) -> Result<Self> {
        let path = config_path(paths)?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }
}

pub fn config_path(paths: &impl PathProvider) -> Result<PathBuf> {
    Ok(paths.config_dir()?.join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::testsupport::FixedPathProvider;

    #[test]
    fn uses_default_config_when_file_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let paths = FixedPathProvider {
            data: temp.path().join("data"),
            config: temp.path().join("config"),
        };

        assert_eq!(Config::load(&paths).unwrap(), Config::default());
    }

    #[test]
    fn resolves_config_path_under_dm_config_dir() {
        let paths = FixedPathProvider {
            data: PathBuf::from("/tmp/data"),
            config: PathBuf::from("/tmp/config/dm"),
        };

        assert_eq!(
            config_path(&paths).unwrap(),
            PathBuf::from("/tmp/config/dm/config.toml")
        );
    }
}
