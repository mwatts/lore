PATH = '/home/mwatts/code/github/mwatts/lore/lore-revision/src/commit_contents.rs'
EDITS = [
('''    let mut changed = false;
    for (path, bytes) in contents.writes {
        changed |= write_file(&repository, &state, &path, bytes).await?;
    }''','''    // A batch of many files looks each name up in a table of its directory's
    // children, built once: a sibling walk per file is quadratic in a directory
    // that holds thousands.
    let mut children = Children::new(contents.writes.len() > CHILD_TABLE_BATCH);
    let mut changed = false;
    for (path, bytes) in contents.writes {
        changed |= write_file(&repository, &state, &mut children, &path, bytes).await?;
    }'''),
('''async fn write_file(
    repository: &Arc<RepositoryContext>,
    state: &Arc<State>,
    path: &str,
    bytes: Bytes,
) -> Result<bool, CommitError> {
    let relative = parse_path(path)?;
    let (parent, name) = ensure_parent(repository, state, &relative).await?;
    let name_hash = crate::hash::hash_string(name);
    let existing = match state
        .find_subnode(repository.clone(), parent, name_hash)
        .await
    {
        Ok(node_id) => Some((''','''async fn write_file(
    repository: &Arc<RepositoryContext>,
    state: &Arc<State>,
    children: &mut Children,
    path: &str,
    bytes: Bytes,
) -> Result<bool, CommitError> {
    let relative = parse_path(path)?;
    let (parent, name) = ensure_parent(repository, state, children, &relative).await?;
    let name_hash = crate::hash::hash_string(name);
    let existing = match children
        .find(repository, state, parent, name_hash)
        .await?
    {
        Some(node_id) => Some(('''),
('''                .forward::<CommitError>("Failed deserializing state node block")?,
        )),
        Err(_) => None,
    };
    if let Some((_, node)) = &existing''','''                .forward::<CommitError>("Failed deserializing state node block")?,
        )),
        None => None,
    };
    if let Some((_, node)) = &existing'''),
('''        None => {
            add_node(
                repository,
                state,
                parent,
                file_node(name, size, address),
                name,
            )
            .await?;
        }
    }
    Ok(true)
}''','''        None => {
            let node_id = add_node(
                repository,
                state,
                parent,
                file_node(name, size, address),
                name,
            )
            .await?;
            children.added(parent, name_hash, node_id);
        }
    }
    Ok(true)
}'''),
('''async fn ensure_parent<'a>(
    repository: &Arc<RepositoryContext>,
    state: &Arc<State>,
    path: &'a RelativePath,
) -> Result<(NodeID, &'a str), CommitError> {
    let mut components: Vec<&str> = path.as_str().split('/').collect();
    let name = components.pop().unwrap_or_default();
    let mut parent = ROOT_NODE;
    for component in components {
        let name_hash = crate::hash::hash_string(component);
        parent = match state
            .find_subnode(repository.clone(), parent, name_hash)
            .await
        {
            Ok(node_id) => {''','''async fn ensure_parent<'a>(
    repository: &Arc<RepositoryContext>,
    state: &Arc<State>,
    children: &mut Children,
    path: &'a RelativePath,
) -> Result<(NodeID, &'a str), CommitError> {
    let mut components: Vec<&str> = path.as_str().split('/').collect();
    let name = components.pop().unwrap_or_default();
    let mut parent = ROOT_NODE;
    for component in components {
        let name_hash = crate::hash::hash_string(component);
        parent = match children
            .find(repository, state, parent, name_hash)
            .await?
        {
            Some(node_id) => {'''),
('''            Err(_) => {
                add_node(
                    repository,
                    state,
                    parent,
                    directory_node(component),
                    component,
                )
                .await?
            }
        };
    }
    Ok((parent, name))
}''','''            None => {
                let node_id = add_node(
                    repository,
                    state,
                    parent,
                    directory_node(component),
                    component,
                )
                .await?;
                children.added(parent, name_hash, node_id);
                node_id
            }
        };
    }
    Ok((parent, name))
}

/// Writes in one commit past which names are looked up in a table per directory.
const CHILD_TABLE_BATCH: usize = 8;

/// Child lookups for one commit: a sibling walk per lookup, or, for a large
/// batch, a table of each directory's children by name hash, built on first use.
struct Children {
    tables: Option<HashMap<NodeID, HashMap<u64, NodeID>>>,
}

impl Children {
    fn new(tabled: bool) -> Self {
        Self {
            tables: tabled.then(HashMap::new),
        }
    }

    /// The child of `parent` named by `name_hash`, as `State::find_subnode` finds
    /// it: the first in sibling order.
    async fn find(
        &mut self,
        repository: &Arc<RepositoryContext>,
        state: &Arc<State>,
        parent: NodeID,
        name_hash: u64,
    ) -> Result<Option<NodeID>, CommitError> {
        let Some(tables) = self.tables.as_mut() else {
            return Ok(state
                .find_subnode(repository.clone(), parent, name_hash)
                .await
                .ok());
        };
        if !tables.contains_key(&parent) {
            let mut table = HashMap::new();
            let parent_node = state
                .node(repository.clone(), parent)
                .await
                .forward::<CommitError>("Failed deserializing state node block")?;
            if parent_node.is_directory() {
                let mut walk =
                    StateNodeChildrenIterator::new(state.clone(), repository.clone(), parent)
                        .await
                        .forward::<CommitError>("Failed deserializing state node block")?;
                while let Some((child_id, child)) = walk
                    .next()
                    .await
                    .forward::<CommitError>("Failed deserializing state node block")?
                {
                    table.entry(child.name_hash).or_insert(child_id);
                }
            }
            tables.insert(parent, table);
        }
        Ok(tables[&parent].get(&name_hash).copied())
    }

    /// Record a node just added under `parent`.
    fn added(&mut self, parent: NodeID, name_hash: u64, node_id: NodeID) {
        if let Some(table) = self.tables.as_mut().and_then(|tables| tables.get_mut(&parent)) {
            table.entry(name_hash).or_insert(node_id);
        }
    }
}'''),
('''use std::sync::Arc;
''','''use std::collections::HashMap;
use std::sync::Arc;
'''),
('''use crate::state::State;
''','''use crate::state::State;
use crate::state::StateNodeChildrenIterator;
'''),
]
