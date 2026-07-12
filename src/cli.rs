use crate::config::AutomationMode;
use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "rem",
    version,
    about = "Local project and cache lifecycle manager"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum AutoCommand {
    /// Show scheduler and automatic cleanup status.
    Status,
    /// Install the platform scheduler for daily automatic cleanup.
    Enable,
    /// Remove the platform scheduler.
    Disable,
    /// Run one automatic cleanup pass immediately.
    Run,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open a repository in a subshell.
    #[command(alias = "o")]
    Open {
        /// Repository id (supports partial match) such as owner/repo or github.com/owner/repo.
        project: Option<String>,
    },
    /// Print repositories to standard output.
    #[command(alias = "l")]
    List {
        /// Include remote repositories from GitHub.
        #[arg(short = 'r', long)]
        remote: bool,

        /// Show cache size and last access age.
        #[arg(short, long)]
        size: bool,

        /// Sort by last access time (most recent first).
        #[arg(short = 'L', long)]
        log: bool,
    },
    /// Mark a repository closed and remove its cache directories.
    #[command(alias = "c")]
    Close {
        /// Repository id (supports partial match) such as owner/repo or github.com/owner/repo.
        project: Option<String>,

        /// Close all activated repositories.
        #[arg(long)]
        all: bool,

        /// Skip confirmation prompts.
        #[arg(long)]
        yes: bool,
    },
    /// Compatibility alias for `gc`.
    Clean {
        /// Clean all managed repositories regardless of age.
        #[arg(long)]
        all: bool,

        /// Skip confirmation prompts.
        #[arg(long)]
        yes: bool,
    },
    /// Scan local repositories and refresh cache inventory.
    Scan,
    /// Apply or preview the cache cleanup policy.
    Gc {
        /// Only show the cleanup plan.
        #[arg(long)]
        dry_run: bool,

        /// Include recently used projects in an explicit cleanup plan.
        #[arg(long)]
        all: bool,

        /// Skip confirmation in ask mode.
        #[arg(long)]
        yes: bool,

        /// Override the configured automation mode.
        #[arg(long, value_enum)]
        mode: Option<AutomationMode>,
    },
    /// Manage scheduler-backed automatic cleanup.
    Auto {
        #[command(subcommand)]
        action: AutoCommand,
    },
    /// Show git status of managed repositories.
    #[command(alias = "s")]
    Status {
        /// Include local (non-activated) repositories.
        #[arg(short, long)]
        all: bool,
    },
    /// Clone and activate a remote repository.
    #[command(alias = "g")]
    Get {
        /// Repository id such as owner/repo or github.com/owner/repo.
        project: Option<String>,
    },
    /// Pull latest changes in activated repositories.
    #[command(alias = "sy")]
    Sync,
    /// Show or edit configuration.
    #[command(alias = "cfg")]
    Config {
        /// Open the config file in $EDITOR.
        #[arg(long, conflicts_with = "update")]
        edit: bool,

        /// Update the config to the current schema, preserving a backup.
        #[arg(long, conflicts_with = "edit")]
        update: bool,
    },
    /// Remove projects whose directories no longer exist.
    #[command(aliases = ["p", "forget"])]
    Prune {
        /// Skip confirmation prompts.
        #[arg(long)]
        yes: bool,
    },
    /// Protect a project from automatic cache cleanup.
    Protect { project: String },
    /// Remove a project from the protected list.
    Unprotect { project: String },
    /// Show cache cleanup history.
    History,
    /// Diagnose local tools and configuration.
    Doctor,
    /// Show project access history.
    #[command(alias = "h")]
    Log,
    /// Generate shell completions and the optional shell wrapper.
    Init {
        /// Shell to generate completions for.
        shell: Shell,
    },
    /// Record the current directory as recently used (for shell integration).
    #[command(hide = true)]
    Touch {
        /// Directory to record; defaults to the current directory.
        path: Option<PathBuf>,

        /// Suppress all output.
        #[arg(long)]
        quiet: bool,
    },
}
