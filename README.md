# repom

`rem` is a context-aware repository manager for local GitHub projects. It uses
GitHub CLI (`gh`) and `ghq` to find or clone repositories, opens them in a
subshell, records access history, and helps clean stale build caches such as
`target` and `node_modules`.

## Requirements

- Rust 1.95+
- `gh` (GitHub CLI)
- `ghq`
- `fzf` optional; `rem` falls back to numbered prompts when it is unavailable

## Shell Integration (Recommended)

Add to your `.zshrc` (or `.bashrc`):

```sh
eval "$(rem init zsh)"
```

Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`.

## Usage

### Dashboard

```
rem
```

Opens the interactive repository dashboard showing managed projects with their
git status (dirty/ahead/behind), cache size, and last access age. Select a
project with `fzf` (or a numbered prompt) to open it.

### Commands

| Command | Alias | Description |
|---|---|---|
| `rem open [project]` | `o` | Open a project in a subshell. Without a project, shows managed projects first, then falls back to GitHub API. |
| `rem list` | `l` | Print repositories. Use `--all` / `-a` to include remote repos from GitHub. |
| `rem status` | `s` | Show git status (dirty/ahead/behind) of all managed projects. |
| `rem sync` | `sy` | Pull latest changes (`--ff-only`) in activated projects. |
| `rem close <project>` | `c` | Close a project, remove cache directories, mark as local. Blocks if uncommitted changes or unpushed commits exist. |
| `rem clean` | — | Remove cache directories (`target`, `node_modules`) from projects older than the configured threshold. `--all` cleans all projects. `--yes` skips confirmation. |
| `rem prune` | `p` | Remove from the project list any projects whose directories no longer exist. |
| `rem log` | `h` | Show project access history sorted by last access time. |
| `rem config` | `cfg` | Show the config file path and contents. `--edit` opens it in `$EDITOR`. |
| `rem init <shell>` | — | Generate shell completion script (`bash`, `zsh`, `fish`, `powershell`, `elvish`). |

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
rem list --all

# Show git status of all managed projects
rem status
rem s

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

# Remove stale projects from the list
rem prune
rem p --yes

# Show access history
rem log
rem h

# Configuration
rem config
rem config --edit

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
cache_targets = ["target", "node_modules"]
older_than = "14d"
```
