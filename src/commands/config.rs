use crate::config::{CURRENT_CONFIG_VERSION, Config, config_path};
use crate::paths::PathProvider;
use anyhow::{Context, Result};
use std::fs;
use std::process::Command as ShellCommand;

pub fn show_config(paths: &impl PathProvider, edit: bool, update: bool) -> Result<()> {
    let path = config_path(paths)?;
    if update {
        return update_config(paths);
    }
    if edit {
        if !path.exists() {
            update_config(paths)?;
        }
        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| "vim".to_string());
        let status = ShellCommand::new(&editor)
            .arg(&path)
            .status()
            .with_context(|| format!("failed to launch {editor}"))?;
        if !status.success() {
            anyhow::bail!("{editor} exited with status {status}");
        }
        return Ok(());
    }

    println!("Config: {}", path.display());
    if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        print!("{raw}");
    } else {
        println!("(default configuration)");
        print!(
            "{}",
            toml::to_string_pretty(&Config::default())
                .context("failed to serialize default configuration")?
        );
        eprintln!(
            "Note: no cache budget is configured by default. Set cleanup.max_cache_size before enabling automatic cleanup."
        );
    }
    Ok(())
}

fn update_config(paths: &impl PathProvider) -> Result<()> {
    let path = config_path(paths)?;
    let existed = path.exists();
    let mut config = Config::load(paths)?;
    if existed && !config.is_outdated() {
        println!(
            "Config is already at schema {CURRENT_CONFIG_VERSION}: {}",
            path.display()
        );
        return Ok(());
    }

    let parent = path.parent().context("config path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let backup = path.with_extension("toml.bak");
    if existed {
        fs::copy(&path, &backup).with_context(|| {
            format!(
                "failed to back up {} to {}",
                path.display(),
                backup.display()
            )
        })?;
    }

    config.config_version = CURRENT_CONFIG_VERSION;
    let raw = toml::to_string_pretty(&config).context("failed to serialize configuration")?;
    let temporary = path.with_extension(format!("toml.{}.tmp", std::process::id()));
    fs::write(&temporary, raw)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, &path)
        .with_context(|| format!("failed to replace {}", path.display()))?;

    println!(
        "Updated config to schema {CURRENT_CONFIG_VERSION}: {}",
        path.display()
    );
    if existed {
        println!("Backup: {}", backup.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::testsupport::FixedPathProvider;

    #[test]
    fn updates_legacy_config_and_preserves_a_backup() {
        let temp = tempfile::tempdir().unwrap();
        let paths = FixedPathProvider {
            data: temp.path().join("data"),
            config: temp.path().join("config"),
        };
        fs::create_dir_all(&paths.config).unwrap();
        let path = paths.config.join("config.toml");
        fs::write(&path, "cache_targets = [\"target\"]\n").unwrap();

        update_config(&paths).unwrap();

        assert_eq!(
            Config::load(&paths).unwrap().config_version,
            CURRENT_CONFIG_VERSION
        );
        assert_eq!(
            fs::read_to_string(path.with_extension("toml.bak")).unwrap(),
            "cache_targets = [\"target\"]\n"
        );
    }
}
