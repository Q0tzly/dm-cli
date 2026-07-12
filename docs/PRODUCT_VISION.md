# repom: Product Vision and Workflow Notes

> Status: discussion draft. This document captures the current product direction and the
> decisions that still need validation before implementation.

## Core problem

Development repositories accumulate large, reproducible directories such as `target`,
`node_modules`, and framework caches. Repositories that are no longer used continue to occupy
storage, while manually finding and cleaning them is tedious and easy to forget.

The primary purpose of `repom` is therefore:

> Safely manage the local lifecycle of development projects and keep their reproducible caches
> within a predictable storage budget.

Repository navigation, cloning, and synchronization are useful, but they support this purpose;
they are not the product's only reason to exist.

## Positioning

`repom` should not reimplement GitHub or Git primitives. `gh`, `ghq`, and `git` already perform
those operations well. However, requiring users to switch between several tools for one project
workflow also creates friction.

The intended boundary is:

- `gh`, `ghq`, and `git` provide low-level capabilities.
- `repom` owns the project-level intent, policy, state, and workflow.
- Users can complete a coherent workflow through `repom`, while `repom` delegates the underlying
  operation to the appropriate tool.

This makes `repom` a local project control plane rather than a replacement for `gh`.

For example, the following commands can share one project resolver and one inventory:

```text
rem open <query>
rem get <query>
rem status <query>
rem sync <query>
rem clean <query>
rem protect <query>
```

Each command should resolve aliases, partial names, local paths, and `owner/repo` identifiers in
the same way. This consistency is where `repom` adds value beyond wrapping individual commands.

## Product layers

### 1. Core: storage lifecycle management

This is the essential layer and should work even if users navigate repositories with plain `cd`,
`zoxide`, an editor, or another tool.

- Discover and inventory local repositories.
- Record when a repository was last used.
- Measure known, reproducible cache directories.
- Calculate reclaimable storage.
- Protect selected projects from automatic cleanup.
- Clean eligible caches according to policy.
- Record what was removed, when, and why.

### 2. Workflow: consistent project operations

This layer provides a single interface for common project actions.

- Find and enter a local project.
- Clone a missing project and optionally enter it.
- Inspect Git and cache status.
- Synchronize one or more projects.
- Forget stale inventory entries without deleting source code.

These operations may delegate to `gh`, `ghq`, and `git`. They should remain separable so that the
storage lifecycle features do not depend on users adopting `rem open` as their only navigation
method.

### 3. Automation: policy-driven maintenance

Automation should be configurable and explainable. `repom` must not silently expand from safe
cache cleanup into source-code or repository deletion.

## Storage policy

An age threshold alone is not enough. Deleting every cache after 14 days can cause unnecessary
rebuilds even when sufficient disk space is available. A high-water/low-water budget is a better
default model.

Example:

```toml
cache_targets = ["target", "node_modules", ".next/cache"]

[cleanup]
max_cache_size = "50GiB"
target_cache_size = "35GiB"
minimum_inactive = "7d"
check_interval = "24h"
active_lease_timeout = "2h"

protected = ["owner/expensive-to-build"]
```

When total managed cache exceeds `max_cache_size`, `repom` selects eligible cache targets in
least recently used order, weighted by reclaim score, until the total reaches
`target_cache_size`. Selection and deletion happen at the configured target level: cleaning
`target` must not remove an unrelated `node_modules` or `.next/cache` directory in the same
project.

The initial reclaim score is deliberately explainable:

```text
reclaim_score = cache_size_bytes × (inactive_days + 1)
```

This allows a large cache that has been unused for a reasonable period to outrank a tiny cache
that has merely been unused for longer. The score is a heuristic, not a claim about the actual
cost of rebuilding a project.

Candidate selection should consider:

- Last use time.
- Cache size.
- Whether the project is protected.
- Whether the project appears to be in use.
- The minimum inactivity period.
- The estimated cost of regenerating a cache, where known.

## Usage detection

Access tracking must not depend only on `rem open`. Otherwise a repository entered with ordinary
`cd` may be incorrectly considered unused.

Shell integration can record directory changes using a lightweight command such as:

```sh
rem touch "$PWD" --quiet
```

Possible hooks:

- zsh: `chpwd`
- fish: `PWD` variable event
- bash: `PROMPT_COMMAND`

The hook should be fast, rate-limited, and should not print during normal shell use. When entering
an untracked GitHub repository, it should add a Local inventory entry rather than losing that usage
signal. It can also trigger a background maintenance check at most once per configured interval;
the start time must be recorded before spawning work so repeated prompts cannot race into several
GC processes.

## Cleanup flow

```text
Repository access is observed
        -> last-used metadata is updated
        -> caches are measured periodically
        -> the storage budget is evaluated
        -> protected, recent, and in-use projects are excluded
        -> cleanup candidates are ranked
        -> the selected automation policy is applied
        -> the decision and reclaimed size are logged
```

The expected daily experience is mostly passive:

```text
$ rem status

Workspace cache: 78.4 GiB / 50 GiB budget
Reclaimable:     34.1 GiB across 12 repositories
Protected:       18.7 GiB
Last cleanup:    2 days ago, reclaimed 21.3 GiB
```

Users can inspect or execute the plan explicitly:

```text
rem scan
rem gc --dry-run
rem gc
rem history
```

## Automation modes

Automation should resemble selectable execution modes in AI tools: the user chooses how much
authority to grant, and `repom` always explains the resulting action.

Suggested global modes:

| Mode | Behavior |
|---|---|
| `off` | Never run maintenance automatically. |
| `suggest` | Detect pressure and show a cleanup plan, but do not modify files. |
| `ask` | Prepare a plan and request approval before cleanup. |
| `auto` | Automatically perform policy-approved cache cleanup. |

A single global switch may be too coarse, so actions should also support per-capability policy:

```toml
[automation]
touch = "auto"
scan = "auto"
clean = "ask"
sync = "ask"
clone = "ask"
forget = "ask"
```

Recommended defaults:

- Read-only inventory and scans: `auto`.
- Access tracking: `auto`.
- Deleting validated, reproducible cache targets: `suggest` initially, then user-enabled `auto`.
- Fast-forward Git synchronization: `ask` or explicit invocation.
- Cloning repositories: `ask` or explicit invocation.
- Forgetting metadata: `ask`.
- Deleting repositories or source files: outside the automatic policy by default.

Every automatic action should support:

- `--dry-run` to show the exact plan.
- A reason describing which policy triggered it.
- An audit/history entry.
- A per-project exclusion or protection mechanism.
- A command-line override such as `--ask`, `--auto`, or `--no-auto` where appropriate.

## Safety principles

- Only explicitly configured relative cache paths may be removed.
- Canonicalized targets must remain inside the repository root.
- A configured target must never be the repository root itself.
- Configured cache targets must not traverse symlinks, even when the resolved path remains inside
  the repository.
- Recently used, protected, or actively leased projects must not be cleaned automatically.
- Source trees and repositories must never be deleted as part of cache cleanup.
- The first automatic-cleanup setup should run in suggestion or dry-run mode.
- Cleanup history must include the project, target, size, time, and triggering policy. Existing
  history entries without target details should remain readable after schema updates.

Uncommitted Git changes are not by themselves a reason to retain a validated build cache. The
stronger safety boundary is proving that only reproducible targets can be removed. This statement
applies only to cache cleanup: lifecycle transitions such as the current `close` command may have
separate safety requirements for uncommitted or unpushed work.

## Existing lifecycle semantics

The current implementation persists an `Activated` or `Local` state and uses `rem close` to bundle
two operations:

1. Transition an activated project back to `Local`.
2. Remove its configured cache directories.

It also blocks the transition when uncommitted changes or unpushed commits are present. This is a
different concern from proving that a configured cache target is safe to delete.

Budget-based automatic garbage collection may make explicit cache removal on every `close`
unnecessary. Before changing the command surface, the product must decide whether to:

- Retain `Activated` / `Local` and keep `close` as an explicit lifecycle operation.
- Retain `close`, but separate the lifecycle transition from optional cache cleanup.
- Replace persistent activation with shell-session leases and let policy-driven GC own cleanup.
- Support both models during a compatibility period.

Until that decision is made, cache cleanup and project lifecycle transitions should be treated as
separate domain operations even if the current command performs both.

## Possible command surface

```text
rem                         Project picker and storage summary
rem open [query]            Enter a project
rem get [query]             Clone and optionally enter a project
rem list                    List known local projects
rem status [query]          Show project and storage status
rem status --all            Show all managed projects
rem sync [query]            Synchronize a project
rem sync --all              Synchronize selected or managed projects
rem scan                    Recalculate cache inventory
rem gc --dry-run            Show the policy cleanup plan
rem gc                      Apply the cleanup plan
rem auto status             Show scheduler and automation status
rem auto enable             Install a platform scheduler
rem auto disable            Remove the platform scheduler
rem auto run                Run one automatic cleanup pass
rem protect <query>         Exclude a project from automatic cleanup
rem unprotect <query>       Remove protection
rem forget <query>          Remove stale metadata, not source files
rem history                 Show maintenance history
rem doctor                  Diagnose hooks, tools, and configuration
rem config edit             Edit configuration
```

The exact names remain open. The important rule is that project resolution and safety behavior
remain consistent across commands.

The shell integration maintains a short-lived lease for the current project session. Leases are
identified by a shell-provided session ID and expire after `active_lease_timeout`; an expired
lease cannot permanently prevent cleanup if a terminal disappears without running an exit hook.

Several proposed names differ from the current CLI (`clean` to `gc`, `prune` to `forget`, and
`log` to `history`). Because these are breaking changes, implementation must choose an explicit
migration policy: preserve the old names as aliases with deprecation messaging, introduce the new
commands alongside them, or make a documented clean break before 1.0.

The first migration keeps `clean` as a compatibility entry point that delegates to `gc`. This
prevents the old command from applying a different cleanup policy while existing scripts and user
habits continue to work.

## Implementation stages

1. Build a reliable local inventory and cache scanner.
2. Add usage tracking that works independently of `rem open`.
3. Implement budget-based planning and `gc --dry-run`.
4. Add protection, audit history, and selectable automation modes.
5. Enable guarded automatic cleanup.
6. Unify project resolution across open/get/status/sync/clean.
7. Add independent background scheduling, regeneration-cost scoring, and provider adapters.

The first scheduler implementation uses launchd on macOS and a systemd user timer on Linux. The
scheduled command is the same `rem auto run` path used for manual verification, so the policy and
safety checks remain centralized. Its cadence is derived from `cleanup.check_interval`, and it is
enabled only after the user explicitly sets `automation.clean = "auto"` and configures a budget.
At this stage the automatic runner is intentionally limited to cache cleanup; policy values for
sync, clone, and metadata removal do not yet authorize those actions automatically.

## Open decisions

- Whether persistent `Activated` / `Local` state remains part of the product model.
- Whether `close` is retained, split into lifecycle and cleanup operations, or replaced by
  session leases plus policy-driven GC.
- How existing command names migrate to the proposed `gc`, `forget`, and `history` vocabulary.
- Whether automatic cleanup is opt-in globally or enabled after an initial dry-run period.
- Whether the lease timeout and prompt heartbeat are appropriate for long-running builds.
- Whether project-specific cache regeneration cost should replace or adjust the initial reclaim
  score.
- Whether `ghq` remains required or repository roots can be configured directly.
- Whether non-GitHub providers should be supported through adapters later.
- Which project operations belong in the coherent `repom` workflow and which should remain direct
  calls to external tools.
