use crate::cli::Cli;
use crate::shell::generate_wrapper;
use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{Shell, generate};

pub fn init_shell(shell: Shell) -> Result<()> {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut std::io::stdout());
    let wrapper = generate_wrapper(&shell);
    if !wrapper.is_empty() {
        print!("{wrapper}");
    }
    Ok(())
}
