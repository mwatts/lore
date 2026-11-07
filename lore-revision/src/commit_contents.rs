// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
//! Commit file contents a caller holds in memory as one revision, with no
//! working tree and no staged state.
//!
//! The working-tree path stages first: `file::stage` reads each file, records it
//! in a staged state, and serializes that state and its anchor, which the commit
//! that follows reads back and replaces. For a caller that already holds the
//! bytes, that staged state is written only to be thrown away. Here the current
//! revision's tree is loaded, the changes are applied to it in memory, and
//! [`commit_in_memory_revision_with`] freezes it with no recovery snapshot, since
//! the tree is rebuilt from the revision on any failure.
//!
//! The revision is published the way a working-tree commit publishes one: the
//! branch tip moves by compare-and-swap against the tip observed at the start,
//! and the instance's current anchor follows it. A crash before the tip moves
//! leaves only unreferenced, content-addressed payloads behind.

use std::sync::Arc;

use bytes::Bytes;
use lore_error_set::prelude::*;

use crate::branch;
use crate::commit::CommitError;
use crate::commit::InMemoryCommitRecovery;
use crate::commit::LoreRevisionCommitRevisionEventData;
use crate::commit::commit_in_memory_revision_with;
use crate::commit::prepare_commit_metadata;
use crate::errors::*;
use crate::event;
use crate::immutable;
use crate::lore::Address;
use crate::lore::Context;
use crate::lore::Hash;
use crate::node::Node;
use crate::node::NodeFlags;
use crate::node::NodeID;
use crate::node::NodeIDExt;
use crate::node::ROOT_NODE;
use crate::repository::RepositoryContext;
use crate::repository::RepositoryWriteToken;
use crate::state::State;
use crate::util::path::RelativePath;

/// One revision's worth of changes, held in memory.
#[derive(Clone, Default)]
pub struct CommitContents {
    /// Repository-relative paths and the complete bytes each should hold.
    pub writes: Vec<(String, Bytes)>,
    /// Repository-relative paths to remove. A path absent from the current
    /// revision is skipped.
    pub removals: Vec<String>,
    /// The revision's message.
    pub message: String,
}

impl std::fmt::Debug for CommitContents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitContents")
            .field(
                "writes",
                &self
                    .writes
                    .iter()
                    .map(|(path, bytes)| (path.as_str(), bytes.len()))
                    .collect::<Vec<_>>(),
            )
            .field("removals", &self.removals)
            .field("message", &self.message)
            .finish()
    }
}

/// Commit `contents` on the instance's current branch as one revision.
///
/// Returns the new revision, or `None` when every write already holds its bytes
/// and every removal is absent, so there is nothing to commit and nothing was
/// published. Fails with `BranchAdvanced` when the branch tip is not the
/// instance's current revision, and with `LocalModifications` when the instance
/// has a staged state, since this commit would publish past it.
///
/// Every write is checked against the current revision before anything is
/// published: a path whose parent is a file, or a write onto a directory, fails
/// the whole commit with nothing published.
pub async fn commit_contents(
    repository: Arc<RepositoryContext>,
    token: &RepositoryWriteToken,
    contents: CommitContents,
) -> Result<Option<Hash>, CommitError> {
    let (current_revision, branch) =
        crate::instance::load_current_anchor(&repository)
            .await
            .forward::<CommitError>("Failed to deserialize current revision anchor")?;
    if let Ok(Some(staged)) = crate::instance::load_staged_revision(&repository).await
        && staged != current_revision
    {
        return Err(LocalModifications.into());
    }
    let observed_tip = branch::load_latest(repository.clone(), branch)
        .await
        .unwrap_or_default();
    if !observed_tip.is_zero() && observed_tip != current_revision {
        return Err(BranchAdvanced.into());
    }

    let state = State::deserialize(repository.clone(), current_revision)
        .await
        .forward_with::<CommitError, _>(|| {
            format!("Failed to deserialize revision state {current_revision}")
        })?;

    let mut changed = false;
    for (path, bytes) in contents.writes {
        changed |= write_file(&repository, &state, &path, bytes).await?;
    }
    for path in &contents.removals {
        changed |= remove_path(&repository, &state, path).await?;
    }
    if !changed {
        return Ok(None);
    }

    let metadata = prepare_commit_metadata(
        repository.clone(),
        crate::metadata::Metadata::new(),
        branch,
        contents.message,
        None,
        None,
        None,
    )
    .await?;

    let signature = commit_in_memory_revision_with(
        repository.clone(),
        token,
        state.clone(),
        (*metadata).clone(),
        current_revision,
        branch,
        InMemoryCommitRecovery::Discard,
    )
    .await
    .map_err(|failure| failure.error)?;

    crate::instance::store_current_anchor(&repository, signature)
        .await
        .forward::<CommitError>("Failed to serialize current revision anchor")?;

    event::LoreEvent::RevisionCommitRevision(LoreRevisionCommitRevisionEventData {
        repository: repository.id,
        branch,
        revision: signature,
        revision_number: state.revision_number(),
        parent: state.parent_self(),
        parent_other: state.parent_other(),
    })
    .send();
    event::metadata::send(&metadata);

    Ok(Some(signature))
}

/// Store `bytes` and point the file node at `path` to them, creating the node
/// and any missing directories. Returns whether the tree changed.
async fn write_file(
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
        Ok(node_id) => Some((
            node_id,
            state
                .node(repository.clone(), node_id)
                .await
                .forward::<CommitError>("Failed deserializing state node block")?,
        )),
        Err(_) => None,
    };
    if let Some((_, node)) = &existing
        && !node.is_file()
    {
        return Err(InvalidPath {
            path: format!("{path} is not a file in the current revision"),
        }
        .into());
    }

    let context = match &existing {
        Some((_, node)) if !node.address.context.is_zero() => node.address.context,
        _ => Context::from(uuid::Uuid::now_v7()),
    };
    let size = bytes.len() as u64;
    let address = immutable::write(
        repository.clone(),
        context,
        bytes,
        immutable::write_options_from_repository(repository.clone()),
    )
    .await
    .forward_with::<CommitError, _>(|| format!("Failed writing file {path} to immutable store"))?;

    match existing {
        Some((node_id, node)) => {
            if node.address.hash == address.hash && node.size == size {
                return Ok(false);
            }
            state
                .node_modify(repository.clone(), node_id, node.mode, size, address)
                .await
                .forward_with::<CommitError, _>(|| format!("Failed to modify {path}"))?;
            let (staged, dirty) = State::staged_edit_flags(&node);
            state
                .node_mark_staged(repository.clone(), node_id, staged, dirty)
                .await
                .forward_with::<CommitError, _>(|| format!("Failed to stage {path}"))?;
        }
        None => {
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
}

/// Stage the node at `path` for deletion. Returns whether the tree changed.
async fn remove_path(
    repository: &Arc<RepositoryContext>,
    state: &Arc<State>,
    path: &str,
) -> Result<bool, CommitError> {
    parse_path(path)?;
    let Ok(link) = state.find_node_link(repository.clone(), path).await else {
        return Ok(false);
    };
    if link.repository != repository.id || !link.node.is_valid_node_id() {
        return Ok(false);
    }
    state
        .node_delete(repository.clone(), link.node)
        .await
        .forward_with::<CommitError, _>(|| format!("Failed to stage the removal of {path}"))
}

/// The directory that holds `path`, created with every missing ancestor, and the
/// name `path` takes in it.
async fn ensure_parent<'a>(
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
            Ok(node_id) => {
                let node = state
                    .node(repository.clone(), node_id)
                    .await
                    .forward::<CommitError>("Failed deserializing state node block")?;
                if !node.is_directory() {
                    return Err(InvalidPath {
                        path: format!("{} has a file where a directory is needed", path.as_str()),
                    }
                    .into());
                }
                node_id
            }
            Err(_) => {
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
}

async fn add_node(
    repository: &Arc<RepositoryContext>,
    state: &Arc<State>,
    parent: NodeID,
    node: Node,
    name: &str,
) -> Result<NodeID, CommitError> {
    let node_id = state
        .node_add(repository.clone(), parent, node, name)
        .await
        .forward_with::<CommitError, _>(|| format!("Failed to add {name}"))?;
    state
        .node_mark_staged(
            repository.clone(),
            node_id,
            NodeFlags::StagedAdd,
            NodeFlags::DirtyAdd,
        )
        .await
        .forward_with::<CommitError, _>(|| format!("Failed to stage {name}"))?;
    Ok(node_id)
}

/// A file node as staging a new working-tree file records it: no executable bit.
fn file_node(name: &str, size: u64, address: Address) -> Node {
    Node {
        flags: NodeFlags::File.bits(),
        mode: 0,
        size,
        address,
        name_hash: crate::hash::hash_string(name),
        ..Default::default()
    }
}

fn directory_node(name: &str) -> Node {
    Node {
        flags: NodeFlags::NoFlags.bits(),
        mode: 0,
        name_hash: crate::hash::hash_string(name),
        ..Default::default()
    }
}

fn parse_path(path: &str) -> Result<RelativePath, CommitError> {
    let relative = RelativePath::new_from_initial_path(path).map_err(|error| -> CommitError {
        InvalidPath {
            path: format!("{path}: {error}"),
        }
        .into()
    })?;
    if relative.is_empty() || relative.as_str() != path {
        return Err(InvalidPath {
            path: format!("{path} is not a clean repository-relative path"),
        }
        .into());
    }
    Ok(relative)
}
