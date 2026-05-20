use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "dm", version, about = "A context-aware repository manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Repository id such as owner/repo or github.com/owner/repo.
    pub project: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open a repository in a subshell.
    Cd {
        /// Repository id such as owner/repo or github.com/owner/repo.
        project: Option<String>,
    },
    /// List managed repositories.
    List,
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
