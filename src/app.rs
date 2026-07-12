use crate::cache::format_bytes;
use crate::cli::{Cli, Command};
use crate::commands;
use crate::config::Config;
use crate::project::ProjectStatus;
use crate::store::ProjectStore;
use anyhow::Result;
use clap::Parser;

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = crate::paths::XdgPathProvider;
    let config = Config::load(&paths)?;
    if crate::config::config_path(&paths)?.exists()
        && let Some(notice) = config.update_notice()
    {
        eprintln!("{notice}");
    }
    let store = ProjectStore::new(&paths)?;

    match cli.command {
        Some(Command::Open { project }) => commands::open::open_project(project, &store),
        Some(Command::List { remote, size, log }) => {
            commands::list::list_projects(&store, &config, remote, size, log)
        }
        Some(Command::Close { project, all, yes }) => {
            commands::close::close_project(&store, &config, project, all, yes)
        }
        Some(Command::Clean { all, yes }) => {
            commands::clean::clean_projects(&store, &config, all, yes)
        }
        Some(Command::Scan) => commands::inventory::scan_command(&store, &config),
        Some(Command::Gc {
            dry_run,
            all,
            yes,
            mode,
        }) => commands::gc::gc_projects(&store, &config, dry_run, all, yes, mode),
        Some(Command::Auto { action }) => commands::auto::run_auto(&store, &config, action),
        Some(Command::Status { all }) => commands::status::status_projects(&store, &config, all),
        Some(Command::Get { project }) => commands::get::get_project(project, &store),
        Some(Command::Sync) => commands::sync::sync_projects(&store),
        Some(Command::Config { edit, update }) => {
            commands::config::show_config(&paths, edit, update)
        }
        Some(Command::Prune { yes }) => commands::prune::prune_projects(&store, yes),
        Some(Command::Protect { project }) => {
            commands::protect::set_protected(&store, &project, true)
        }
        Some(Command::Unprotect { project }) => {
            commands::protect::set_protected(&store, &project, false)
        }
        Some(Command::History) => commands::history::show_history(&store),
        Some(Command::Doctor) => commands::doctor::run_doctor(&paths, &config),
        Some(Command::Log) => commands::log::log_projects(&store, &config),
        Some(Command::Init { shell }) => commands::init::init_shell(shell),
        Some(Command::Touch { path, quiet }) => {
            commands::touch::touch_project(&store, &config, path, quiet)
        }
        None => dashboard_projects(&store, &config),
    }
}

fn dashboard_projects(store: &ProjectStore, config: &Config) -> Result<()> {
    let projects = store.load()?;
    let total = projects.len();
    let activated = projects
        .iter()
        .filter(|project| project.status == ProjectStatus::Activated)
        .count();

    println!();

    let total_cache = projects
        .iter()
        .map(|project| project.cache_size_bytes.unwrap_or(0))
        .sum::<u64>();
    match config.cleanup.max_cache_size.as_deref() {
        Some(maximum) => println!(
            "Workspace cache: {} / {} budget",
            format_bytes(total_cache),
            maximum
        ),
        None => println!("Workspace cache: {}", format_bytes(total_cache)),
    }
    if let Some(last_cleanup) = store.load_history()?.last() {
        println!(
            "Last cleanup: {} ({})",
            last_cleanup.completed_at.format("%Y-%m-%d %H:%M"),
            format_bytes(last_cleanup.reclaimed_bytes)
        );
    }
    println!();
    println!("          ██████╗ ███████╗██████╗  ██████╗ ███╗   ███╗");
    println!("          ██╔══██╗██╔════╝██╔══██╗██╔═══██╗████╗ ████║");
    println!("          ██████╔╝█████╗  ██████╔╝██║   ██║██╔████╔██║");
    println!("          ██╔══██╗██╔══╝  ██╔══╝  ██║   ██║██║╚██╔╝██║");
    println!("          ██║  ██║███████╗██║     ╚██████╔╝██║ ╚═╝ ██║");
    println!("          ╚═╝  ╚═╝╚══════╝╚═╝      ╚═════╝ ╚═╝     ╚═╝");
    println!();

    if total == 0 {
        println!("No projects yet.");
        println!();
        println!("Usage:");
        println!("  rem open <owner/repo>  Open a project");
        println!("  rem o <owner/repo>     Short alias for open");
        println!("  rem list               List all projects");
        println!("  rem scan               Refresh cache inventory");
        println!("  rem gc --dry-run       Preview cache cleanup");
        println!("  rem help               Show all commands");
    } else {
        println!("Projects: {total} total, {activated} activated");
        println!();
        println!("Usage:");
        println!("  rem open                Select and open a project");
        println!("  rem list                Show detailed project list");
        println!("  rem gc --dry-run        Preview cache cleanup");
        println!("  rem status              Show git status of all projects");
        println!("  rem help                Show all commands");
    }

    println!();
    Ok(())
}
