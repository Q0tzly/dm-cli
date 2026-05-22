use clap::{Parser, Subcommand};
use clap_complete::Shell;

#[derive(Debug, Parser)]
#[command(
    name = "rem",
    version,
    about = "Open the interactive repository dashboard, or run a repository command"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open a repository in a subshell.
    #[command(alias = "o")]
    Open {
        /// Repository id (supports partial match) such as owner/repo or github.com/owner/repo.
        project: Option<String>,

        /// Include remote repositories from GitHub.
        #[arg(short, long)]
        all: bool,
    },
    /// Open a repository in a subshell.
    #[command(hide = true)]
    Cd {
        /// Repository id such as owner/repo or github.com/owner/repo.
        project: Option<String>,
    },
    /// Print repositories to standard output.
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
        /// Clean all managed repositories regardless of age.
        #[arg(long)]
        all: bool,

        /// Skip confirmation prompts.
        #[arg(long)]
        yes: bool,
    },
    /// Show git status of managed repositories.
    #[command(alias = "s")]
    Status,
    /// Pull latest changes in activated repositories.
    #[command(alias = "sy")]
    Sync,
    /// Show or edit configuration.
    #[command(alias = "cfg")]
    Config {
        /// Open the config file in $EDITOR.
        #[arg(long)]
        edit: bool,
    },
    /// Remove projects whose directories no longer exist.
    #[command(alias = "p")]
    Prune {
        /// Skip confirmation prompts.
        #[arg(long)]
        yes: bool,
    },
    /// Show project access history.
    #[command(alias = "h")]
    Log,
    /// Generate shell completion script.
    Init {
        /// Shell to generate completions for.
        shell: Shell,
    },
}
