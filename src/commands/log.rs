use crate::config::Config;
use crate::store::ProjectStore;
use anyhow::Result;

pub fn log_projects(store: &ProjectStore, config: &Config) -> Result<()> {
    // Reuse the list_projects function with log=true
    crate::commands::list::list_projects(store, config, false, false, true)
}
