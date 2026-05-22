use anyhow::{Context, Result};
use std::path::PathBuf;

pub trait PathProvider {
    fn data_dir(&self) -> Result<PathBuf>;
    fn config_dir(&self) -> Result<PathBuf>;
}

#[derive(Debug, Clone, Copy)]
pub struct XdgPathProvider;

impl PathProvider for XdgPathProvider {
    fn data_dir(&self) -> Result<PathBuf> {
        base_dir("XDG_DATA_HOME", ".local/share").map(|path| path.join("repom"))
    }

    fn config_dir(&self) -> Result<PathBuf> {
        base_dir("XDG_CONFIG_HOME", ".config").map(|path| path.join("repom"))
    }
}

fn base_dir(env_key: &str, fallback: &str) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(env_key).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    dirs::home_dir()
        .map(|home| home.join(fallback))
        .with_context(|| format!("could not determine home directory for {env_key}"))
}

#[cfg(test)]
pub mod testsupport {
    use super::PathProvider;
    use anyhow::Result;
    use std::path::PathBuf;

    #[derive(Debug, Clone)]
    pub struct FixedPathProvider {
        pub data: PathBuf,
        pub config: PathBuf,
    }

    impl PathProvider for FixedPathProvider {
        fn data_dir(&self) -> Result<PathBuf> {
            Ok(self.data.clone())
        }

        fn config_dir(&self) -> Result<PathBuf> {
            Ok(self.config.clone())
        }
    }
}
