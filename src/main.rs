mod app;
mod cache;
mod cli;
mod commands;
mod config;
mod duration;
mod git;
mod github;
mod paths;
mod project;
mod scheduler;
mod select;
mod shell;
mod store;

use anyhow::Result;

fn main() -> Result<()> {
    app::run()
}
