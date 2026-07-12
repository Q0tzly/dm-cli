use crate::cache::parse_bytes;
use crate::cli::AutoCommand;
use crate::commands::gc::gc_projects;
use crate::config::{AutomationMode, Config};
use crate::duration::parse_age;
use crate::scheduler;
use crate::store::ProjectStore;
use anyhow::{Context, Result};
use std::env;

pub fn run_auto(store: &ProjectStore, config: &Config, action: AutoCommand) -> Result<()> {
    match action {
        AutoCommand::Status => show_status(config),
        AutoCommand::Enable => enable(config),
        AutoCommand::Disable => disable(),
        AutoCommand::Run => gc_projects(
            store,
            config,
            false,
            false,
            true,
            Some(AutomationMode::Auto),
        ),
    }
}

fn show_status(config: &Config) -> Result<()> {
    let status = scheduler::status()?;
    println!("Scheduler: {}", status.name);
    println!("Installed: {}", if status.installed { "yes" } else { "no" });
    for path in &status.paths {
        println!("  {}", path.display());
    }
    println!("Cleanup mode: {:?}", config.automation.clean);
    println!("Check interval: {}", config.cleanup.check_interval);
    match config.cleanup.max_cache_size.as_deref() {
        Some(size) => println!("Cache budget: {}", size),
        None => println!("Cache budget: not configured"),
    }
    Ok(())
}

fn enable(config: &Config) -> Result<()> {
    if config.automation.clean != AutomationMode::Auto {
        anyhow::bail!("set automation.clean = \"auto\" before enabling the automatic scheduler");
    }
    let Some(max_cache_size) = config.cleanup.max_cache_size.as_deref() else {
        anyhow::bail!("configure cleanup.max_cache_size before enabling automatic cleanup");
    };
    parse_bytes(max_cache_size)?;
    let interval_seconds = parse_age(&config.cleanup.check_interval)?.num_seconds();
    if interval_seconds <= 0 {
        anyhow::bail!("cleanup.check_interval must be greater than zero");
    }
    let executable = env::current_exe().context("failed to determine rem executable")?;
    let paths = scheduler::enable(&executable, interval_seconds as u64)?;
    println!("Installed automatic cleanup scheduler:");
    for path in paths {
        println!("  {}", path.display());
    }
    Ok(())
}

fn disable() -> Result<()> {
    let paths = scheduler::disable()?;
    println!("Removed automatic cleanup scheduler files:");
    for path in paths {
        println!("  {}", path.display());
    }
    Ok(())
}
