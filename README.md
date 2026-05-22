# dm-cli

`dm` is a context-aware repository manager for local GitHub projects. It uses
GitHub CLI (`gh`) and `ghq` to find or clone repositories, opens them in a
subshell, records access history, and helps clean stale build caches such as
`target` and `node_modules`.

## Requirements

- Rust 1.95+
- `gh` (GitHub CLI)
- `ghq`
- `fzf` optional; `dm` falls back to numbered prompts when it is unavailable

## Shell Integration (Recommended)

Add to your `.zshrc` (or `.bashrc`):

```sh
eval "$(dm init zsh)"
```

Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`.

## Usage

### Dashboard

```
dm
```

Opens the interactive repository dashboard showing managed projects with their
git status (dirty/ahead/behind), cache size, and last access age. Select a
project with `fzf` (or a numbered prompt) to open it.

### Commands

| Command | Alias | Description |
|---|---|---|
| `dm open [project]` | `o` | Open a project in a subshell. Without a project, shows managed projects first, then falls back to GitHub API. |
| `dm list` | `l` | Print repositories. Use `--all` / `-a` to include remote repos from GitHub. |
| `dm status` | `s` | Show git status (dirty/ahead/behind) of all managed projects. |
| `dm sync` | `sy` | Pull latest changes (`--ff-only`) in activated projects. |
| `dm close <project>` | `c` | Close a project, remove cache directories, mark as local. Blocks if uncommitted changes or unpushed commits exist. |
| `dm clean` | — | Remove cache directories (`target`, `node_modules`) from projects older than the configured threshold. `--all` cleans all projects. `--yes` skips confirmation. |
| `dm prune` | `p` | Remove from the project list any projects whose directories no longer exist. |
| `dm log` | `h` | Show project access history sorted by last access time. |
| `dm config` | `cfg` | Show the config file path and contents. `--edit` opens it in `$EDITOR`. |
| `dm init <shell>` | — | Generate shell completion script (`bash`, `zsh`, `fish`, `powershell`, `elvish`). |

### Examples

```sh
# Open the interactive dashboard
dm

# Open a project (managed list first, then GitHub API)
dm open
dm open owner/repo
dm o owner/repo

# List projects
dm list
dm l
dm list --all

# Show git status of all managed projects
dm status
dm s

# Sync activated projects
dm sync
dm sy

# Close a project (blocks on uncommitted/unpushed changes)
dm close owner/repo
dm c owner/repo --yes

# Clean cache directories
dm clean
dm clean --all
dm clean --all --yes

# Remove stale projects from the list
dm prune
dm p --yes

# Show access history
dm log
dm h

# Configuration
dm config
dm config --edit

# Shell init
eval "$(dm init bash)"
```

### What happens when you open a project

Opening a project starts a subshell in the repository directory, marks it as
**Activated**, and injects the `DM_PROJECT=owner/repo` environment variable.

Closing a project marks it **Local** again and removes configured cache
directories from the filesystem.

### Safety

Before opening another project, `dm` warns about uncommitted changes in the
current directory.

Before closing a project, `dm` blocks entirely if there are uncommitted changes
or unpushed commits. Commit, stash, or push before closing.

`dm` runs `ghq` from your home directory so relative `ghq.root` values such as
`dev` resolve to `~/dev`, not the directory where `dm` was launched.

## Data And Config

- **Project history**: `~/.local/share/dm/projects.json`
- **User config**: `~/.config/dm/config.toml`

Example config:

```toml
cache_targets = ["target", "node_modules"]
older_than = "14d"
```
