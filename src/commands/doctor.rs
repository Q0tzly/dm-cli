use crate::cache::parse_bytes;
use crate::config::{AutomationMode, CURRENT_CONFIG_VERSION, Config};
use crate::duration::parse_age;
use crate::paths::XdgPathProvider;
use crate::select::command_exists;
use anyhow::Result;
use std::path::Path;

pub fn run_doctor(paths: &XdgPathProvider, config: &Config) -> Result<()> {
    let mut failures = 0;
    for (name, required) in [("ghq", true), ("gh", false), ("fzf", false)] {
        let available = command_exists(name);
        println!(
            "{}  {}{}",
            if available { "✓" } else { "✗" },
            name,
            if required && !available {
                " (required)"
            } else if !required && !available {
                " (optional)"
            } else {
                ""
            }
        );
        if required && !available {
            failures += 1;
        }
    }

    let config_path = crate::config::config_path(paths)?;
    println!("Config: {}", config_path.display());
    for issue in config_issues(config) {
        println!("✗ {issue}");
        failures += 1;
    }

    let scheduler = crate::scheduler::status()?;
    println!(
        "Scheduler: {} ({})",
        scheduler.name,
        if scheduler.installed {
            "installed"
        } else {
            "not installed"
        }
    );
    if scheduler.installed && config.automation.clean != AutomationMode::Auto {
        println!("✗ automatic scheduler is installed but clean is not set to auto");
        failures += 1;
    }

    if failures > 0 {
        anyhow::bail!("doctor found {failures} required issue(s)");
    }
    println!("✓ configuration looks valid");
    Ok(())
}

fn config_issues(config: &Config) -> Vec<String> {
    let mut issues = Vec::new();
    if config.is_outdated() {
        issues.push(format!(
            "config schema {} is outdated; run `rem config --update` for schema {}",
            config.config_version, CURRENT_CONFIG_VERSION
        ));
    }
    if config.cache_targets.is_empty() {
        issues.push("cache_targets must include at least one relative path".to_string());
    }
    for target in &config.cache_targets {
        let path = Path::new(target);
        if target.trim().is_empty()
            || path == Path::new(".")
            || path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            issues.push(format!("unsafe cache target: {target}"));
        }
    }

    let maximum = config
        .cleanup
        .max_cache_size
        .as_deref()
        .map(parse_bytes)
        .transpose();
    let target = config
        .cleanup
        .target_cache_size
        .as_deref()
        .map(parse_bytes)
        .transpose();
    match &maximum {
        Ok(None) => {
            if config.automation.clean == AutomationMode::Auto {
                issues.push("clean = auto requires cleanup.max_cache_size".to_string());
            }
        }
        Err(error) => issues.push(format!("invalid cleanup.max_cache_size: {error}")),
        _ => {}
    }
    if let Err(error) = &target {
        issues.push(format!("invalid cleanup.target_cache_size: {error}"));
    }
    if let (Ok(Some(maximum)), Ok(Some(target))) = (maximum, target)
        && target > maximum
    {
        issues.push("cleanup.target_cache_size must not exceed max_cache_size".to_string());
    }

    for (name, value, positive) in [
        (
            "cleanup.minimum_inactive",
            &config.cleanup.minimum_inactive,
            false,
        ),
        (
            "cleanup.check_interval",
            &config.cleanup.check_interval,
            true,
        ),
        (
            "cleanup.active_lease_timeout",
            &config.cleanup.active_lease_timeout,
            true,
        ),
    ] {
        match parse_age(value) {
            Ok(duration) if positive && duration.num_seconds() <= 0 => {
                issues.push(format!("{name} must be greater than zero"))
            }
            Ok(_) => {}
            Err(error) => issues.push(format!("invalid {name}: {error}")),
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_unsafe_and_inconsistent_cleanup_configuration() {
        let config = Config {
            cache_targets: vec![".".to_string()],
            cleanup: crate::config::CleanupConfig {
                max_cache_size: Some("1GiB".to_string()),
                target_cache_size: Some("2GiB".to_string()),
                check_interval: "0h".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let issues = config_issues(&config);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("unsafe cache target"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("target_cache_size"))
        );
        assert!(issues.iter().any(|issue| issue.contains("check_interval")));
    }
}
