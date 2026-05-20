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
dm owner/repo
dm cd owner/repo
dm clean
dm clean --older-than 14d
dm clean --older-than 2w --yes
```

Opening a project starts a subshell in the repository directory and injects
`DM_PROJECT=owner/repo`.

## Data And Config

- Project history: `~/.local/share/dm/projects.json`
- User config: `~/.config/dm/config.toml`

Example config:

```toml
cache_targets = ["target", "node_modules"]
older_than = "14d"
```
