use clap::{CommandFactory, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "dm", version, about = "A context-aware repository manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open a repository in a subshell.
    #[command(alias = "o")]
    Open {
        /// Repository id such as owner/repo or github.com/owner/repo.
        project: Option<String>,
    },
    /// Open a repository in a subshell.
    #[command(hide = true)]
    Cd {
        /// Repository id such as owner/repo or github.com/owner/repo.
        project: Option<String>,
    },
    /// List the repository dashboard.
    #[command(alias = "l")]
    List {
        /// Include remote repositories that have not been opened locally.
        #[arg(short, long)]
        all: bool,
    },
    /// Mark a repository closed and remove its cache directories.
    #[command(alias = "c")]
    Close {
        /// Repository id such as owner/repo or github.com/owner/repo.
        project: String,

        /// Skip confirmation prompts.
        #[arg(long)]
        yes: bool,
    },
    /// Remove cache directories from managed repositories.
    Clean {
        /// Clean projects not accessed within this age, for example 14d or 2w.
        #[arg(long)]
        older_than: Option<String>,

        /// Skip confirmation prompts.
        #[arg(long)]
        yes: bool,
    },
}

pub fn print_help() -> anyhow::Result<()> {
    let mut command = Cli::command();
    command.print_help()?;
    println!();
    Ok(())
}
