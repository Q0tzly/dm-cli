use crate::cache::{
    clean_cache_target, format_bytes, parse_bytes, scan_cache_target_size, scan_project_cache_size,
};
use crate::commands::inventory::scan_projects;
use crate::commands::util::confirm;
use crate::config::{AutomationMode, Config};
use crate::duration::parse_age;
use crate::project::Project;
use crate::store::{CleanupHistoryEntry, CleanupHistoryTarget, ProjectStore};
use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::HashSet;

#[derive(Debug, Clone)]
struct Candidate {
    index: usize,
    target: String,
    label: String,
    bytes: u64,
    last_accessed_at: chrono::DateTime<Utc>,
    inactive_days: i64,
    score: u128,
}

pub fn gc_projects(
    store: &ProjectStore,
    config: &Config,
    dry_run: bool,
    all: bool,
    yes: bool,
    requested_mode: Option<AutomationMode>,
) -> Result<()> {
    let mode = requested_mode.unwrap_or(config.automation.clean);
    if mode == AutomationMode::Off {
        anyhow::bail!("automatic cleanup is disabled; use --mode ask or --mode suggest");
    }

    if mode == AutomationMode::Auto && all {
        anyhow::bail!("--all cannot be used with automatic cleanup");
    }

    let mut projects = scan_projects(store, config)?;
    let total = projects
        .iter()
        .map(|project| project.cache_size_bytes.unwrap_or(0))
        .sum::<u64>();
    let max_cache_size = config
        .cleanup
        .max_cache_size
        .as_deref()
        .map(parse_bytes)
        .transpose()?;
    let target_cache_size = config
        .cleanup
        .target_cache_size
        .as_deref()
        .map(parse_bytes)
        .transpose()?
        .or(max_cache_size);

    if mode == AutomationMode::Auto && max_cache_size.is_none() {
        anyhow::bail!("automatic cleanup requires cleanup.max_cache_size to be configured");
    }

    if let (Some(maximum), Some(target)) = (max_cache_size, target_cache_size) {
        if target > maximum {
            anyhow::bail!("target_cache_size must not exceed max_cache_size");
        }
    }

    if !all
        && let Some(maximum) = max_cache_size
        && total <= maximum
    {
        println!(
            "Cache is within budget: {} / {}.",
            format_bytes(total),
            format_bytes(maximum)
        );
        record_automatic_check(store, mode, "cache budget not exceeded")?;
        return Ok(());
    }

    let minimum_inactive = parse_age(&config.cleanup.minimum_inactive)?;
    let cutoff = Utc::now() - minimum_inactive;
    let now = Utc::now();
    let lease_timeout = parse_age(&config.cleanup.active_lease_timeout)?;
    let active_project_ids = store.active_project_ids(lease_timeout)?;
    let mut candidates = Vec::new();
    for (index, project) in projects.iter().enumerate() {
        if is_protected(project, config)
            || active_project_ids.contains(&project.id)
            || !project.path.exists()
            || (!all && project.last_accessed_at >= cutoff)
        {
            continue;
        }

        for target in &config.cache_targets {
            let bytes = scan_cache_target_size(&project.path, target)
                .with_context(|| format!("failed to scan {}/{}", project.id, target))?;
            if bytes == 0 {
                continue;
            }

            let inactive_days = now
                .signed_duration_since(project.last_accessed_at)
                .num_days()
                .max(0);
            candidates.push(Candidate {
                index,
                target: target.clone(),
                label: format!("{}/{}", project.owner_repo(), target),
                bytes,
                last_accessed_at: project.last_accessed_at,
                inactive_days,
                score: u128::from(bytes) * (inactive_days as u128 + 1),
            });
        }
    }
    rank_candidates(&mut candidates);

    let selected = select_candidates(&candidates, total, target_cache_size, all);
    if selected.is_empty() {
        println!("No eligible cache cleanup candidates.");
        record_automatic_check(store, mode, "no eligible cleanup candidates")?;
        return Ok(());
    }

    let selected_bytes: u64 = selected.iter().map(|candidate| candidate.bytes).sum();
    println!(
        "Cleanup plan: {} cache target(s), reclaiming up to {}.",
        selected.len(),
        format_bytes(selected_bytes)
    );
    for candidate in &selected {
        println!(
            "  {}  {}  last used {}",
            candidate.label,
            format_bytes(candidate.bytes),
            candidate.last_accessed_at.format("%Y-%m-%d")
        );
        println!(
            "      inactive: {}d, reclaim score: {}",
            candidate.inactive_days, candidate.score
        );
    }

    if dry_run || mode == AutomationMode::Suggest {
        println!("Dry run: no files were removed.");
        return Ok(());
    }

    if mode == AutomationMode::Ask && !yes && !confirm("Apply this cleanup plan? [y/N] ")? {
        println!("Cancelled.");
        return Ok(());
    }

    let mut reclaimed_bytes = 0;
    let mut touched_projects = HashSet::new();
    let mut cleaned_targets = Vec::new();
    for candidate in &selected {
        let project = &projects[candidate.index];
        if !project.path.exists() {
            continue;
        }
        let reclaimed = clean_cache_target(&project.path, &candidate.target)
            .with_context(|| format!("failed to clean {}/{}", project.id, candidate.target))?;
        reclaimed_bytes += reclaimed;
        touched_projects.insert(candidate.index);
        cleaned_targets.push(CleanupHistoryTarget {
            label: candidate.label.clone(),
            bytes: reclaimed,
        });
    }

    for index in touched_projects {
        let project = &mut projects[index];
        project.cache_size_bytes = Some(
            scan_project_cache_size(&project.path, &config.cache_targets)
                .with_context(|| format!("failed to rescan {}", project.id))?,
        );
    }

    store.save(&projects)?;
    store.append_history(CleanupHistoryEntry {
        completed_at: Utc::now(),
        mode: format_mode(mode),
        reason: if max_cache_size.is_some() {
            "cache budget exceeded".to_string()
        } else {
            "explicit cleanup".to_string()
        },
        reclaimed_bytes,
        targets: cleaned_targets,
        projects: Vec::new(),
    })?;
    println!("Reclaimed {}.", format_bytes(reclaimed_bytes));
    Ok(())
}

fn record_automatic_check(store: &ProjectStore, mode: AutomationMode, reason: &str) -> Result<()> {
    if mode == AutomationMode::Auto {
        store.append_history(CleanupHistoryEntry {
            completed_at: Utc::now(),
            mode: format_mode(mode),
            reason: reason.to_string(),
            reclaimed_bytes: 0,
            targets: Vec::new(),
            projects: Vec::new(),
        })?;
    }
    Ok(())
}

fn select_candidates(
    candidates: &[Candidate],
    total: u64,
    target_cache_size: Option<u64>,
    all: bool,
) -> Vec<Candidate> {
    if all || target_cache_size.is_none() {
        return candidates.to_vec();
    }

    let target = target_cache_size.unwrap_or(total);
    let mut remaining = total;
    let mut selected = Vec::new();
    for candidate in candidates {
        if remaining <= target {
            break;
        }
        remaining = remaining.saturating_sub(candidate.bytes);
        selected.push(candidate.clone());
    }
    selected
}

fn rank_candidates(candidates: &mut [Candidate]) {
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.last_accessed_at.cmp(&right.last_accessed_at))
    });
}

fn is_protected(project: &Project, config: &Config) -> bool {
    project.protected
        || config
            .cleanup
            .protected
            .iter()
            .any(|value| value == &project.id || value == project.owner_repo())
}

fn format_mode(mode: AutomationMode) -> String {
    match mode {
        AutomationMode::Off => "off",
        AutomationMode::Suggest => "suggest",
        AutomationMode::Ask => "ask",
        AutomationMode::Auto => "auto",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_large_old_cache_before_small_old_cache() {
        let now = Utc::now();
        let mut candidates = vec![
            Candidate {
                index: 0,
                target: "target".to_string(),
                label: "small".to_string(),
                bytes: 1,
                last_accessed_at: now - chrono::Duration::days(100),
                inactive_days: 100,
                score: 101,
            },
            Candidate {
                index: 1,
                target: "target".to_string(),
                label: "large".to_string(),
                bytes: 10,
                last_accessed_at: now - chrono::Duration::days(20),
                inactive_days: 20,
                score: 210,
            },
        ];

        rank_candidates(&mut candidates);
        let selected = select_candidates(&candidates, 11, Some(1), false);
        assert_eq!(selected[0].label, "large");
    }
}
