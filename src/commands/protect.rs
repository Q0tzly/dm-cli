use crate::store::ProjectStore;
use anyhow::Result;

pub fn set_protected(store: &ProjectStore, query: &str, protected: bool) -> Result<()> {
    let mut projects = store.load()?;
    let query = query.trim().to_ascii_lowercase();
    let matches: Vec<usize> = projects
        .iter()
        .enumerate()
        .filter(|(_, project)| {
            project.id.to_ascii_lowercase().contains(&query)
                || project.owner_repo().to_ascii_lowercase().contains(&query)
        })
        .map(|(index, _)| index)
        .collect();

    let index = match matches.as_slice() {
        [index] => *index,
        [] => anyhow::bail!("no projects found matching '{query}'"),
        _ => anyhow::bail!("query '{query}' matches multiple projects; use a more specific query"),
    };

    let project = &mut projects[index];
    project.protected = protected;
    let label = project.owner_repo().to_string();
    store.save(&projects)?;

    if protected {
        println!("Protected {label} from automatic cleanup.");
    } else {
        println!("Removed protection from {label}.");
    }
    Ok(())
}
