use crate::commands::util::confirm;
use crate::store::ProjectStore;
use anyhow::Result;

pub fn prune_projects(store: &ProjectStore, yes: bool) -> Result<()> {
    let mut projects = store.load()?;
    let stale: Vec<_> = projects
        .iter()
        .filter(|p| !p.path.exists())
        .cloned()
        .collect();

    if stale.is_empty() {
        println!("No stale projects to prune.");
        return Ok(());
    }

    println!("Stale projects (directory no longer exists):");
    for p in &stale {
        println!("  {}  {}", p.owner_repo(), p.path.display());
    }

    if !yes && !confirm("Remove them from the project list? [y/N] ")? {
        println!("Cancelled.");
        return Ok(());
    }

    let stale_ids: std::collections::HashSet<_> = stale.iter().map(|p| p.id.clone()).collect();
    projects.retain(|p| !stale_ids.contains(&p.id));
    store.save(&projects)?;

    println!("Pruned {} project(s).", stale.len());
    Ok(())
}
