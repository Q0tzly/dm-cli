use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub fn open_subshell(project_id: &str, path: &Path) -> Result<()> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let _status = Command::new(&shell)
        .current_dir(path)
        .env("REM_PROJECT", project_id)
        .status()
        .with_context(|| format!("failed to start shell {shell}"))?;

    Ok(())
}

pub fn generate_wrapper(shell: &clap_complete::Shell) -> &'static str {
    match shell {
        clap_complete::Shell::Bash | clap_complete::Shell::Zsh => {
            "rem() {\n  if [[ \"$1\" == \"close\" || \"$1\" == \"c\" ]]; then\n    command rem \"$@\"\n    local ret=$?\n    if [[ $ret -eq 0 ]]; then\n      builtin cd ~\n    fi\n    return $ret\n  else\n    command rem \"$@\"\n  fi\n}\n"
        }
        clap_complete::Shell::Fish => {
            "function rem --wraps rem\n  if test \"$argv[1]\" = close -o \"$argv[1]\" = c\n    command rem $argv\n    and cd ~\n  else\n    command rem $argv\n  end\nend\n"
        }
        _ => "",
    }
}
