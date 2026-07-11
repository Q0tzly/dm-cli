use crate::cache::progress_bar;
use crate::git::pull_ff_only;
use crate::project::ProjectStatus;
use crate::store::ProjectStore;
use anyhow::Result;

pub fn sync_projects(store: &ProjectStore) -> Result<()> {
    let projects = store.load()?;
    let activated: Vec<_> = projects
        .iter()
        .filter(|p| p.status == ProjectStatus::Activated)
        .collect();

    if activated.is_empty() {
        println!("No activated projects to sync.");
        return Ok(());
    }

    let bar = progress_bar("Syncing", activated.len() as u64);

    let results: Vec<(String, Result<bool>)> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for project in &activated {
            let name = project.owner_repo().to_string();
            let path = project.path.clone();
            handles.push(s.spawn(move || (name, pull_ff_only(&path))));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut ok = 0u32;
    let mut fail = 0u32;
    for (name, result) in &results {
        match result {
            Ok(true) => {
                ok += 1;
                println!("  ↑ {name}");
            }
            Ok(false) => {
                println!("  · {name}");
            }
            Err(e) => {
                fail += 1;
                println!("  ✗ {name}: {e}");
            }
        }
        bar.inc(1);
    }
    bar.finish_and_clear();

    let parts = vec![
        (ok > 0).then(|| format!("{ok} updated")),
        (fail > 0).then(|| format!("{fail} failed")),
    ];
    let summary: Vec<_> = parts.into_iter().flatten().collect();
    if summary.is_empty() {
        println!("All up to date.");
    } else {
        println!("{}", summary.join(", "));
    }

    Ok(())
}
