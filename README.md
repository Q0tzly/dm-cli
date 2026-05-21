# dm-cli

`dm` is a context-aware repository manager for local GitHub projects. It uses
GitHub CLI (`gh`) and `ghq` to find or clone repositories, opens them in a
subshell, records access history, and helps clean stale build caches such as
`target` and `node_modules`.

## Requirements

- Rust 1.95+
- `gh`
- `ghq`
- `fzf` optional; `dm` falls back to numbered prompts when it is unavailable

## Usage

```sh
dm
dm list
dm l
dm list --all
dm open owner/repo
dm o owner/repo
dm close owner/repo
dm c owner/repo --yes
dm clean
dm clean --older-than 14d
dm clean --older-than 2w --yes
```

Running `dm` without arguments opens the interactive repository dashboard. Use
`dm list` when you only want to print the repository list to the shell. Opening
a project starts a subshell in the repository directory, marks it activated, and
injects `DM_PROJECT=owner/repo`. Closing a project marks it local again and
removes configured cache directories.

`dm` runs `ghq` from your home directory so relative `ghq.root` values such as
`dev` resolve to `~/dev`, not the directory where `dm` was launched. Before
opening, closing, or cleaning a repository, `dm` warns when the relevant git
worktree has unstaged or untracked changes.

## Data And Config

- Project history: `~/.local/share/dm/projects.json`
- User config: `~/.config/dm/config.toml`

Example config:

```toml
cache_targets = ["target", "node_modules"]
older_than = "14d"
```
