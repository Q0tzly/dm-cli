use anyhow::{Context, Result, bail};
use std::io::{self, Write};
use std::process::{Command, Stdio};

pub fn choose_one(prompt: &str, choices: &[String]) -> Result<Option<String>> {
    if choices.is_empty() {
        return Ok(None);
    }

    if command_exists("fzf") {
        return choose_with_fzf(prompt, choices, false).map(|items| items.into_iter().next());
    }

    choose_one_numbered(prompt, choices)
}

pub fn choose_many(prompt: &str, choices: &[String]) -> Result<Vec<String>> {
    if choices.is_empty() {
        return Ok(Vec::new());
    }

    if command_exists("fzf") {
        return choose_with_fzf(prompt, choices, true);
    }

    choose_many_numbered(prompt, choices)
}

pub fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn choose_with_fzf(prompt: &str, choices: &[String], multi: bool) -> Result<Vec<String>> {
    let mut command = Command::new("fzf");
    command.arg("--prompt").arg(format!("{prompt}> "));
    if multi {
        command.arg("--multi");
    }

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to start fzf")?;

    {
        let stdin = child.stdin.as_mut().context("failed to open fzf stdin")?;
        for choice in choices {
            writeln!(stdin, "{choice}").context("failed to write fzf choices")?;
        }
    }

    let output = child.wait_with_output().context("failed to wait for fzf")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let selected = String::from_utf8(output.stdout).context("fzf output was not UTF-8")?;
    Ok(selected
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn choose_one_numbered(prompt: &str, choices: &[String]) -> Result<Option<String>> {
    print_numbered(prompt, choices)?;
    print!("Select one number, or press enter to cancel: ");
    io::stdout().flush().context("failed to flush stdout")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read selection")?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let index = parse_number(trimmed, choices.len())?;
    Ok(Some(choices[index].clone()))
}

fn choose_many_numbered(prompt: &str, choices: &[String]) -> Result<Vec<String>> {
    print_numbered(prompt, choices)?;
    print!("Select numbers separated by commas, or press enter to cancel: ");
    io::stdout().flush().context("failed to flush stdout")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read selections")?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    trimmed
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| parse_number(item, choices.len()).map(|index| choices[index].clone()))
        .collect()
}

fn print_numbered(prompt: &str, choices: &[String]) -> Result<()> {
    println!("{prompt}");
    for (index, choice) in choices.iter().enumerate() {
        println!("{:>3}. {choice}", index + 1);
    }
    Ok(())
}

fn parse_number(input: &str, len: usize) -> Result<usize> {
    let number: usize = input
        .parse()
        .with_context(|| format!("invalid selection {input:?}"))?;
    if number == 0 || number > len {
        bail!("selection {number} is outside 1..={len}");
    }
    Ok(number - 1)
}
