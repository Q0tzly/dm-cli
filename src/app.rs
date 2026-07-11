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
        Some(Command::Status { all }) => commands::status::status_projects(&store, &config, all),
        Some(Command::Get { project }) => commands::get::get_project(project, &store),
        Some(Command::Sync) => commands::sync::sync_projects(&store),
        Some(Command::Config { edit }) => commands::config::show_config(&paths, edit),
        Some(Command::Prune { yes }) => commands::prune::prune_projects(&store, yes),
        Some(Command::Log) => commands::log::log_projects(&store, &config),
        Some(Command::Init { shell }) => commands::init::init_shell(shell),
        None => dashboard_projects(&store),
    }
}

fn dashboard_projects(store: &ProjectStore) -> Result<()> {
    let projects = store.load()?;
    let total = projects.len();
    let activated = projects
        .iter()
        .filter(|project| project.status == ProjectStatus::Activated)
        .count();

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
        println!("  rem help               Show all commands");
    } else {
        println!("Projects: {total} total, {activated} activated");
        println!();
        println!("Usage:");
        println!("  rem open                Select and open a project");
        println!("  rem list                Show detailed project list");
        println!("  rem status              Show git status of all projects");
        println!("  rem help                Show all commands");
    }

    println!();
    Ok(())
}
