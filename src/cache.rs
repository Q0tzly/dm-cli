use anyhow::{Context, Result, bail};
use ignore::WalkBuilder;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub fn scan_project_cache_size(project_root: &Path, targets: &[String]) -> Result<u64> {
    let total = Arc::new(AtomicU64::new(0));

    for target in targets {
        let target_path = validate_cache_target(project_root, target)?;
        if !target_path.exists() {
            continue;
        }

        let walker = WalkBuilder::new(target_path)
            .hidden(false)
            .ignore(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .threads(4)
            .build_parallel();

        let total_for_walk = Arc::clone(&total);
        walker.run(|| {
            let total_for_entry = Arc::clone(&total_for_walk);
            Box::new(move |entry| {
                if let Ok(entry) = entry
                    && let Ok(metadata) = entry.metadata()
                    && metadata.is_file()
                {
                    total_for_entry.fetch_add(metadata.len(), Ordering::Relaxed);
                }
                ignore::WalkState::Continue
            })
        });
    }

    Ok(total.load(Ordering::Relaxed))
}

pub fn validate_cache_target(project_root: &Path, target: &str) -> Result<PathBuf> {
    let relative = Path::new(target);
    if relative.is_absolute() {
        bail!("cache target {target:?} must be relative");
    }

    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("cache target {target:?} must not contain ..");
    }

    let root = fs::canonicalize(project_root)
        .with_context(|| format!("failed to canonicalize {}", project_root.display()))?;
    let candidate = root.join(relative);
    let canonical = if candidate.exists() {
        fs::canonicalize(&candidate)
            .with_context(|| format!("failed to canonicalize {}", candidate.display()))?
    } else {
        candidate
    };

    if !canonical.starts_with(&root) {
        bail!(
            "cache target {} resolves outside project root {}",
            canonical.display(),
            root.display()
        );
    }

    Ok(canonical)
}

pub fn clean_project(project_root: &Path, targets: &[String]) -> Result<u64> {
    let mut removed_bytes = 0;

    for target in targets {
        let target_path = validate_cache_target(project_root, target)?;
        if !target_path.exists() {
            continue;
        }

        let size = scan_project_cache_size(project_root, std::slice::from_ref(target))?;
        if target_path.is_dir() {
            fs::remove_dir_all(&target_path)
                .with_context(|| format!("failed to remove {}", target_path.display()))?;
        } else {
            fs::remove_file(&target_path)
                .with_context(|| format!("failed to remove {}", target_path.display()))?;
        }
        removed_bytes += size;
    }

    Ok(removed_bytes)
}

pub fn progress_bar(message: &str, len: u64) -> ProgressBar {
    let bar = ProgressBar::new(len);
    bar.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg} [{elapsed_precise}] {pos}/{len}")
            .unwrap()
            .tick_strings(&["-", "\\", "|", "/"]),
    );
    bar.set_message(message.to_string());
    bar.enable_steady_tick(Duration::from_millis(120));
    bar
}

pub fn progress_spinner(message: &str) -> ProgressBar {
    let bar = ProgressBar::new_spinner();
    bar.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg} [{elapsed_precise}]")
            .unwrap()
            .tick_strings(&["-", "\\", "|", "/"]),
    );
    bar.set_message(message.to_string());
    bar.enable_steady_tick(Duration::from_millis(120));
    bar
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn validates_safe_relative_cache_target() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("target")).unwrap();

        assert_eq!(
            validate_cache_target(temp.path(), "target").unwrap(),
            fs::canonicalize(temp.path().join("target")).unwrap()
        );
    }

    #[test]
    fn rejects_unsafe_cache_targets() {
        let temp = tempfile::tempdir().unwrap();

        assert!(validate_cache_target(temp.path(), "/tmp").is_err());
        assert!(validate_cache_target(temp.path(), "../outside").is_err());
    }

    #[test]
    fn scans_cache_size_for_configured_targets() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("target")).unwrap();
        let mut file = fs::File::create(temp.path().join("target/artifact.bin")).unwrap();
        file.write_all(&[0; 1024]).unwrap();

        let size = scan_project_cache_size(temp.path(), &["target".to_string()]).unwrap();

        assert_eq!(size, 1024);
    }

    #[test]
    fn clean_removes_only_configured_cache_targets() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("target")).unwrap();
        fs::write(temp.path().join("target/artifact.bin"), [0; 16]).unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[package]\n").unwrap();

        let removed = clean_project(temp.path(), &["target".to_string()]).unwrap();

        assert_eq!(removed, 16);
        assert!(!temp.path().join("target").exists());
        assert!(temp.path().join("Cargo.toml").exists());
    }
}
