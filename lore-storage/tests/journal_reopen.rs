// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
//! What a journaled store leaves on disk for the next open: a store closed
//! before its first fold reopens with the bucket layout a new store has, and a
//! journaled store's info file names a version that a binary which cannot
//! replay the journal refuses.

use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use lore_storage::Address;
use lore_storage::Context;
use lore_storage::Fragment;
use lore_storage::ImmutableStore;
use lore_storage::ImmutableStoreSettings;
use lore_storage::LocalImmutableStore;
use lore_storage::Partition;

const FRAGMENTS: u64 = 2_000;

fn partition() -> Partition {
    Partition::from([3u8; 16])
}

fn payload(n: u64) -> Bytes {
    Bytes::from(format!("fragment {n} of a journaled store").into_bytes())
}

fn address(n: u64) -> Address {
    Address {
        hash: lore_storage::hash_slice(&payload(n)),
        context: Context::default(),
    }
}

async fn open(root: &Path, journal: bool) -> Arc<LocalImmutableStore> {
    LocalImmutableStore::new(
        Some(root.to_path_buf()),
        ImmutableStoreSettings {
            journal,
            ..Default::default()
        },
    )
    .await
    .expect("the immutable store opens")
}

/// Store `FRAGMENTS` small fragments and flush them into the journal, without a fold.
async fn write_and_close(root: &Path) {
    let store = open(root, true).await;
    for n in 0..FRAGMENTS {
        let payload = payload(n);
        let fragment = Fragment {
            flags: 0,
            size_payload: payload.len() as u32,
            size_content: payload.len() as u64,
        };
        store
            .clone()
            .put(partition(), address(n), fragment, Some(payload), false)
            .await
            .expect("put succeeds");
    }
    store.clone().flush(true).await.expect("the flush succeeds");
    assert!(
        std::fs::metadata(root.join("immutable").join("journal")).is_ok_and(|m| m.len() > 0),
        "the flush left the fragments in the journal, unfolded"
    );
}

async fn reads_back(store: &Arc<LocalImmutableStore>) -> u64 {
    let mut found = 0;
    for n in 0..FRAGMENTS {
        if let Ok(got) = store.clone().get(partition(), address(n)).await
            && got.payload.as_ref() == Some(&payload(n))
        {
            found += 1;
        }
    }
    found
}

/// Bucket files under each group directory of the store's index, by group.
fn bucket_files(root: &Path) -> Vec<(String, usize, bool)> {
    let index = root.join("immutable").join("index");
    let mut groups = Vec::new();
    for group in std::fs::read_dir(&index)
        .expect("the index exists")
        .flatten()
    {
        let names: Vec<String> = std::fs::read_dir(group.path())
            .expect("a group directory reads")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        let buckets = names
            .iter()
            .filter(|name| name.starts_with("index_"))
            .count();
        let marked = names.iter().any(|name| name == "level");
        groups.push((
            group.file_name().to_string_lossy().into_owned(),
            buckets,
            marked,
        ));
    }
    groups
}

/// A journaled store writes no bucket before its first fold, so a store closed before
/// then holds group directories with no bucket in them. The next open must read that as
/// a new store, whose groups start at one bucket and record their level, and not as one
/// written before bucket levels existed, whose groups stay at 256 buckets each for good:
/// every fold of such a store rewrites a bucket file per 256th of a group.
#[tokio::test]
async fn a_store_closed_before_its_first_fold_reopens_with_a_new_stores_layout() {
    let dir = lore_base::test_util::TempDir::new("lore-journal-reopen-layout-");
    let root = dir.path().join("store");
    write_and_close(&root).await;

    let store = open(&root, true).await;
    assert_eq!(
        reads_back(&store).await,
        FRAGMENTS,
        "the reopen replays the journal"
    );
    drop(store);

    let groups = bucket_files(&root);
    let written: Vec<_> = groups
        .iter()
        .filter(|(_, buckets, _)| *buckets > 0)
        .collect();
    assert!(!written.is_empty(), "the reopen's fold wrote the buckets");
    for (group, buckets, marked) in written {
        assert!(
            *marked && *buckets == 1,
            "group {group} holds {buckets} bucket files and {} level marker, as a new store's \
             group does not",
            if *marked { "a" } else { "no" }
        );
    }
}

/// A binary without the journal reads only the packfiles and buckets, so it would open a
/// journaled store and go without what the journal holds. The only info file version such
/// a binary accepts is 0; a journaled store's says otherwise before the journal takes a
/// record. A binary with the journal still opens it, journaled or not, and replays it.
#[tokio::test]
async fn a_journaled_store_names_a_version_older_binaries_refuse() {
    let dir = lore_base::test_util::TempDir::new("lore-journal-reopen-version-");
    let root = dir.path().join("store");
    write_and_close(&root).await;

    let info = std::fs::read(root.join("immutable").join("info")).expect("the info file reads");
    let version = u32::from_ne_bytes(info[4..8].try_into().expect("4 bytes"));
    assert_ne!(
        version, 0,
        "the info file names a version a binary that cannot replay the journal refuses"
    );

    let store = open(&root, false).await;
    assert_eq!(
        reads_back(&store).await,
        FRAGMENTS,
        "an open without the journal setting still replays it"
    );
}
