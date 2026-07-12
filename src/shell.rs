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
        clap_complete::Shell::Bash => {
            "rem() {\n  if [[ \"$1\" == \"close\" || \"$1\" == \"c\" ]]; then\n    command rem \"$@\"\n    local ret=$?\n    if [[ $ret -eq 0 ]]; then\n      builtin cd ~\n    fi\n    return $ret\n  else\n    command rem \"$@\"\n  fi\n}\n\n_rem_touch() { REPOM_SESSION_ID=$$ command rem touch --quiet >/dev/null 2>&1 & }\nif [[ -z \"${REM_TOUCH_HOOK:-}\" ]]; then\n  REM_TOUCH_HOOK=1\n  PROMPT_COMMAND=\"_rem_touch${PROMPT_COMMAND:+;$PROMPT_COMMAND}\"\nfi\n"
        }
        clap_complete::Shell::Zsh => {
            "rem() {\n  if [[ \"$1\" == \"close\" || \"$1\" == \"c\" ]]; then\n    command rem \"$@\"\n    local ret=$?\n    if [[ $ret -eq 0 ]]; then\n      builtin cd ~\n    fi\n    return $ret\n  else\n    command rem \"$@\"\n  fi\n}\n\nif (( ! ${+functions[_rem_touch]} )); then\n  _rem_touch() { REPOM_SESSION_ID=$$ command rem touch --quiet >/dev/null 2>&1 & }\n  autoload -Uz add-zsh-hook\n  add-zsh-hook precmd _rem_touch\nfi\n_rem_touch\n"
        }
        clap_complete::Shell::Fish => {
            "function rem --wraps rem\n  if test \"$argv[1]\" = close -o \"$argv[1]\" = c\n    command rem $argv\n    and cd ~\n  else\n    command rem $argv\n  end\nend\n\nfunction _rem_touch --on-event fish_prompt\n  env REPOM_SESSION_ID=$fish_pid command rem touch --quiet >/dev/null 2>&1 &\nend\n_rem_touch\n"
        }
        _ => "",
    }
}
