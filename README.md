# repom

`rem` is a local project and cache lifecycle manager for GitHub projects. It uses
GitHub CLI (`gh`) and `ghq` for repository operations while keeping reproducible
build caches such as `target` and `node_modules` within a storage budget.

## Requirements

- Rust 1.85+
- `gh` (GitHub CLI)
- `ghq`
- `fzf` optional; `rem` falls back to numbered prompts when it is unavailable

## Shell Integration (Recommended)

Add to your `.zshrc` (or `.bashrc`):

```sh
eval "$(rem init zsh)"
```

Completions support `bash`, `zsh`, `fish`, `powershell`, and `elvish`. The optional usage-tracking
hook is currently provided for `bash`, `zsh`, and `fish`.

## Usage

### Dashboard

```
rem
```

Shows an ASCII dashboard with the number of tracked and activated projects and
the main commands. Use `rem list --size` or `rem status` for detailed git,
cache, and access information, then `rem open` to select a local repository.

### Commands

| Command | Alias | Description |
|---|---|---|
| `rem open [project]` | `o` | Open a project in a subshell. Without a project, shows managed projects first, then falls back to GitHub API. |
| `rem list` | `l` | Print repositories. Use `--remote` / `-r` to include remote repos from GitHub. `--size` / `-s` adds cache, age, and git counts; `--log` / `-L` sorts by recent access. |
| `rem status` | `s` | Show cache budget, reclaimable targets, and Git status (dirty/ahead/behind) of managed projects. |
| `rem get [project]` | `g` | Clone and activate a remote repository. |
| `rem sync` | `sy` | Pull latest changes (`--ff-only`) in activated projects. |
| `rem close <project>` | `c` | Close a project, remove cache directories, mark as local. Blocks if uncommitted changes or unpushed commits exist. |
| `rem close --all` | — | Close all activated projects. |
| `rem clean` | — | Compatibility alias for `gc`. `--all` includes recently used projects and `--yes` skips confirmation. |
| `rem scan` | — | Discover local repositories and refresh cache sizes. |
| `rem gc` | — | Preview or apply the budget-based cache cleanup policy. `--dry-run`, `--mode`, and `--yes` control execution. |
| `rem auto status` | — | Show scheduler and automatic cleanup status. |
| `rem auto enable` | — | Install the platform scheduler using `cleanup.check_interval`. Requires `clean = "auto"` and a cache budget. |
| `rem auto disable` | — | Remove the platform scheduler files. |
| `rem auto run` | — | Run one automatic cleanup pass immediately. |
| `rem protect [project]` | — | Exclude a project from automatic cache cleanup. |
| `rem unprotect [project]` | — | Remove a project from the protected list. |
| `rem history` | — | Show cache cleanup history, including target and reclaimed size. |
| `rem doctor` | — | Check local tools and cache configuration. |
| `rem prune` | `p`, `forget` | Remove from the project list any projects whose directories no longer exist. |
| `rem log` | `h` | Show project access history sorted by last access time. |
| `rem config` | `cfg` | Show config. `--edit` opens it; `--update` migrates it with a backup. |
| `rem init <shell>` | — | Generate shell completions and the `close` wrapper (`bash`, `zsh`, `fish`, `powershell`, `elvish`). |

### Examples

```sh
# Open the interactive dashboard
rem

# Open a project (managed list first, then GitHub API)
rem open
rem open owner/repo
rem o owner/repo

# List projects
rem list
rem l
rem list --remote
rem list --size
rem list --log

# Show git status of all managed projects
rem status
rem s

# Clone and activate a remote project
rem get owner/repo
rem g owner/repo

# Sync activated projects
rem sync
rem sy

# Close a project (blocks on uncommitted/unpushed changes)
rem close owner/repo
rem c owner/repo --yes

# Clean cache directories
rem clean
rem clean --all
rem clean --all --yes

# Inspect and manage the cache budget
rem scan
rem gc --dry-run
rem gc
rem gc --mode auto
rem auto status
rem auto enable
rem protect owner/large-project
rem history

# Remove stale projects from the list
rem prune
rem forget --yes

# Show access history
rem log
rem h

# Configuration
rem config
rem config --edit
rem config --update

# Shell init
eval "$(rem init bash)"
```

### What happens when you open a project

Opening a project starts a subshell in the repository directory, marks it as
**Activated**, and injects the `REM_PROJECT=owner/repo` environment variable.

Closing a project marks it **Local** again and removes configured cache
directories from the filesystem.

### Safety

Before opening another project, `rem` warns about uncommitted changes in the
current directory.

Before closing a project, `rem` blocks entirely if there are uncommitted changes
or unpushed commits. Commit, stash, or push before closing.

`rem` runs `ghq` from your home directory so relative `ghq.root` values such as
`dev` resolve to `~/dev`, not the directory where `rem` was launched.

## Data And Config

- **Project history**: `~/.local/share/repom/projects.json`
- **User config**: `~/.config/repom/config.toml`

Example config:

```toml
config_version = 1
cache_targets = ["target", "node_modules"]
older_than = "14d"

[cleanup]
max_cache_size = "50GiB"
target_cache_size = "35GiB"
minimum_inactive = "14d"
check_interval = "24h"
active_lease_timeout = "2h"
protected = ["owner/large-project"]

[automation]
touch = "auto"
scan = "auto"
clean = "ask"
sync = "ask"
clone = "ask"
forget = "ask"
```

`config_version` tracks the configuration schema rather than the application release. When a new
repom version requires a config migration, commands print an update notice. Run
`rem config --update` to merge new defaults into the existing values; the previous file is saved as
`config.toml.bak`. A missing config file can also be materialized with the same command.

The current automatic runner performs cache cleanup only. The other capability modes reserve a
consistent policy surface for future project operations and do not trigger network or metadata
changes by themselves.

Use `rem init zsh` (or the equivalent shell) to install completion and lightweight usage
tracking hooks. The hooks record repository access without requiring navigation through `rem`,
including a previously untracked GitHub repository entered with ordinary `cd`. Metadata and lease
heartbeats are rate-limited to one write per minute for each shell session.

Automatic cleanup is opt-in. Begin with `rem gc --dry-run`, then configure both a budget and
`clean = "auto"` before running `rem auto enable`. The scheduler uses `check_interval`.

`rem clean` remains available for compatibility, but uses the same budget, protection, lease, and
reclaim-score logic as `rem gc`.

Cleanup plans operate on individual configured cache targets, so removing a project’s `target`
does not also remove its `node_modules` or other unrelated cache directories.

Configured cache targets must be non-empty relative paths below the project root; `.` and paths
containing `..` or traversing symbolic links are rejected. `rem doctor` validates these and other
cleanup-policy errors.

`rem auto enable` installs and activates a launchd job on macOS or a systemd user timer on Linux.
`rem auto disable` removes the corresponding scheduler.
