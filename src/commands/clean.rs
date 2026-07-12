use crate::commands::gc::gc_projects;
use crate::config::{AutomationMode, Config};
use crate::store::ProjectStore;
use anyhow::Result;

/// Backward-compatible entry point for the budget-based garbage collector.
pub fn clean_projects(store: &ProjectStore, config: &Config, all: bool, yes: bool) -> Result<()> {
    gc_projects(store, config, false, all, yes, Some(AutomationMode::Ask))
}
