mod app;
mod cache;
mod cli;
mod config;
mod duration;
mod github;
mod paths;
mod project;
mod select;
mod shell;
mod store;

use anyhow::Result;

fn main() -> Result<()> {
    app::run()
}
