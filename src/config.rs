use crate::paths::PathProvider;
use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default = "default_cache_targets")]
    pub cache_targets: Vec<String>,
    #[serde(default = "default_older_than")]
    pub older_than: String,
    #[serde(default)]
    pub cleanup: CleanupConfig,
    #[serde(default)]
    pub automation: AutomationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupConfig {
    #[serde(default)]
    pub max_cache_size: Option<String>,
    #[serde(default)]
    pub target_cache_size: Option<String>,
    #[serde(default = "default_minimum_inactive")]
    pub minimum_inactive: String,
    #[serde(default = "default_check_interval")]
    pub check_interval: String,
    #[serde(default = "default_active_lease_timeout")]
    pub active_lease_timeout: String,
    #[serde(default)]
    pub protected: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AutomationMode {
    Off,
    Suggest,
    #[default]
    Ask,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationConfig {
    #[serde(default)]
    pub touch: AutomationMode,
    #[serde(default)]
    pub scan: AutomationMode,
    #[serde(default)]
    pub clean: AutomationMode,
    #[serde(default)]
    pub sync: AutomationMode,
    #[serde(default)]
    pub clone: AutomationMode,
    #[serde(default)]
    pub forget: AutomationMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cache_targets: default_cache_targets(),
            older_than: default_older_than(),
            cleanup: CleanupConfig::default(),
            automation: AutomationConfig::default(),
        }
    }
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            max_cache_size: None,
            target_cache_size: None,
            minimum_inactive: default_minimum_inactive(),
            check_interval: default_check_interval(),
            active_lease_timeout: default_active_lease_timeout(),
            protected: Vec::new(),
        }
    }
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            touch: AutomationMode::Auto,
            scan: AutomationMode::Auto,
            clean: AutomationMode::Ask,
            sync: AutomationMode::Ask,
            clone: AutomationMode::Ask,
            forget: AutomationMode::Ask,
        }
    }
}

pub fn default_cache_targets() -> Vec<String> {
    vec!["target".to_string(), "node_modules".to_string()]
}

fn default_older_than() -> String {
    "14d".to_string()
}

fn default_minimum_inactive() -> String {
    "14d".to_string()
}

fn default_check_interval() -> String {
    "24h".to_string()
}

fn default_active_lease_timeout() -> String {
    "2h".to_string()
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
    fn resolves_config_path_under_repom_config_dir() {
        let paths = FixedPathProvider {
            data: PathBuf::from("/tmp/data"),
            config: PathBuf::from("/tmp/config/dm"),
        };

        assert_eq!(
            config_path(&paths).unwrap(),
            PathBuf::from("/tmp/config/dm/config.toml")
        );
    }

    #[test]
    fn loads_cleanup_budget_and_automation_mode() {
        let config: Config = toml::from_str(
            r#"
cache_targets = ["target"]

[cleanup]
max_cache_size = "50GiB"
target_cache_size = "35GiB"
minimum_inactive = "7d"

[automation]
clean = "auto"
"#,
        )
        .unwrap();

        assert_eq!(config.cleanup.max_cache_size.as_deref(), Some("50GiB"));
        assert_eq!(config.cleanup.minimum_inactive, "7d");
        assert_eq!(config.automation.clean, AutomationMode::Auto);
    }
}
