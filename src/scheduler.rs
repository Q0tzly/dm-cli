use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchedulerKind {
    Launchd,
    SystemdUser,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct SchedulerStatus {
    pub name: &'static str,
    pub paths: Vec<PathBuf>,
    pub installed: bool,
}

pub fn status() -> Result<SchedulerStatus> {
    let kind = scheduler_kind();
    let paths = scheduler_paths(kind)?;
    let installed = !paths.is_empty() && paths.iter().all(|path| path.exists());
    Ok(SchedulerStatus {
        name: scheduler_name(kind),
        paths,
        installed,
    })
}

pub fn enable(executable: &Path, interval_seconds: u64) -> Result<Vec<PathBuf>> {
    if interval_seconds == 0 {
        bail!("automatic scheduler interval must be greater than zero")
    }
    let kind = scheduler_kind();
    let paths = scheduler_paths(kind)?;
    match kind {
        SchedulerKind::Launchd => {
            write_launchd(&paths[0], executable, interval_seconds)?;
            activate_launchd(&paths[0])?;
        }
        SchedulerKind::SystemdUser => {
            write_systemd(&paths, executable, interval_seconds)?;
            activate_systemd()?
        }
        SchedulerKind::Unsupported => {
            bail!("automatic scheduler is unsupported on this platform")
        }
    }
    Ok(paths)
}

pub fn disable() -> Result<Vec<PathBuf>> {
    let kind = scheduler_kind();
    let paths = scheduler_paths(kind)?;
    match kind {
        SchedulerKind::Launchd if !paths.is_empty() => deactivate_launchd(&paths[0]),
        SchedulerKind::SystemdUser => deactivate_systemd(),
        SchedulerKind::Unsupported => {}
        _ => {}
    }
    for path in &paths {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("failed to remove {}", path.display()));
            }
        }
    }
    Ok(paths)
}

fn scheduler_kind() -> SchedulerKind {
    if cfg!(target_os = "macos") {
        SchedulerKind::Launchd
    } else if cfg!(target_os = "linux") {
        SchedulerKind::SystemdUser
    } else {
        SchedulerKind::Unsupported
    }
}

fn scheduler_name(kind: SchedulerKind) -> &'static str {
    match kind {
        SchedulerKind::Launchd => "launchd",
        SchedulerKind::SystemdUser => "systemd user timer",
        SchedulerKind::Unsupported => "unsupported",
    }
}

fn scheduler_paths(kind: SchedulerKind) -> Result<Vec<PathBuf>> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(match kind {
        SchedulerKind::Launchd => vec![home.join("Library/LaunchAgents/com.repom.gc.plist")],
        SchedulerKind::SystemdUser => vec![
            home.join(".config/systemd/user/repom-gc.service"),
            home.join(".config/systemd/user/repom-gc.timer"),
        ],
        SchedulerKind::Unsupported => Vec::new(),
    })
}

fn write_launchd(path: &Path, executable: &Path, interval_seconds: u64) -> Result<()> {
    let parent = path.parent().context("launchd path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let executable = xml_escape(&executable.display().to_string());
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.repom.gc</string>
  <key>ProgramArguments</key>
  <array>
    <string>{executable}</string>
    <string>auto</string>
    <string>run</string>
  </array>
  <key>StartInterval</key>
  <integer>{interval_seconds}</integer>
  <key>RunAtLoad</key>
  <false/>
  <key>StandardOutPath</key>
  <string>/tmp/repom-auto.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/repom-auto.log</string>
</dict>
</plist>
"#
    );
    fs::write(path, plist).with_context(|| format!("failed to write {}", path.display()))
}

fn write_systemd(paths: &[PathBuf], executable: &Path, interval_seconds: u64) -> Result<()> {
    let parent = paths
        .first()
        .and_then(|path| path.parent())
        .context("systemd path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let service = format!(
        "[Unit]\nDescription=repom automatic cache cleanup\n\n[Service]\nType=oneshot\nExecStart=\"{}\" auto run\n",
        executable.display()
    );
    let timer = format!(
        "[Unit]\nDescription=Run repom automatic cache cleanup\n\n[Timer]\nOnBootSec=15min\nOnUnitActiveSec={interval_seconds}s\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
    );
    fs::write(&paths[0], service)
        .with_context(|| format!("failed to write {}", paths[0].display()))?;
    fs::write(&paths[1], timer).with_context(|| format!("failed to write {}", paths[1].display()))
}

fn activate_launchd(path: &Path) -> Result<()> {
    let domain = launchd_domain()?;
    let _ = Command::new("launchctl")
        .args(["bootout", &domain, &path.display().to_string()])
        .status();
    run_command(
        Command::new("launchctl").args(["bootstrap", &domain, &path.display().to_string()]),
        "failed to bootstrap launchd job",
    )
}

fn deactivate_launchd(path: &Path) {
    if let Ok(domain) = launchd_domain() {
        let _ = Command::new("launchctl")
            .args(["bootout", &domain, &path.display().to_string()])
            .status();
    }
}

fn activate_systemd() -> Result<()> {
    run_command(
        Command::new("systemctl").args(["--user", "daemon-reload"]),
        "failed to reload systemd user units",
    )?;
    run_command(
        Command::new("systemctl").args(["--user", "enable", "--now", "repom-gc.timer"]),
        "failed to enable repom systemd timer",
    )
}

fn deactivate_systemd() {
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", "repom-gc.timer"])
        .status();
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
}

fn launchd_domain() -> Result<String> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .context("failed to determine current user id")?;
    if !output.status.success() {
        bail!("failed to determine current user id");
    }
    let uid = String::from_utf8(output.stdout).context("user id was not UTF-8")?;
    Ok(format!("gui/{}", uid.trim()))
}

fn run_command(command: &mut Command, context: &str) -> Result<()> {
    let output = command.output().with_context(|| context.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("{context}: {stderr}");
    }
    Ok(())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_xml_values() {
        assert_eq!(xml_escape("a&b<\"c\""), "a&amp;b&lt;&quot;c&quot;");
    }

    #[test]
    fn scheduler_name_is_stable() {
        assert_eq!(scheduler_name(SchedulerKind::Launchd), "launchd");
        assert_eq!(
            scheduler_name(SchedulerKind::SystemdUser),
            "systemd user timer"
        );
    }

    #[test]
    fn systemd_timer_uses_configured_interval() {
        let temp = tempfile::tempdir().unwrap();
        let paths = vec![
            temp.path().join("repom-gc.service"),
            temp.path().join("repom-gc.timer"),
        ];

        write_systemd(&paths, Path::new("/usr/local/bin/rem"), 3_600).unwrap();

        assert!(
            fs::read_to_string(&paths[1])
                .unwrap()
                .contains("OnUnitActiveSec=3600s")
        );
    }
}
