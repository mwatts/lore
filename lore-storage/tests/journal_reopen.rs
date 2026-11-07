// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
//! What a journaled store leaves on disk for the next open: a store closed
//! before its first fold reopens with the bucket layout a new store has, and a
//! journaled store's info file names a version that a binary which cannot
//! replay the journal refuses. A journaled open replays the journal into
//! memory and writes nothing; the journal stays until a later fold.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use bytes::Bytes;
use lore_base::types::KeyType;
use lore_storage::Address;
use lore_storage::Context;
use lore_storage::Fragment;
use lore_storage::Hash;
use lore_storage::ImmutableStore;
use lore_storage::ImmutableStoreSettings;
use lore_storage::LocalImmutableStore;
use lore_storage::LocalMutableStore;
use lore_storage::MutableStore;
use lore_storage::MutableStoreSettings;
use lore_storage::Partition;
use lore_storage::local::immutable_store::JOURNAL_FOLD_SIZE;

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

/// Store fragments `range` and flush them into the journal, without a fold.
async fn put_and_flush(store: &Arc<LocalImmutableStore>, range: std::ops::Range<u64>) {
    for n in range {
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
}

/// Store `FRAGMENTS` small fragments and flush them into the journal, without a fold.
async fn write_and_close(root: &Path) {
    let store = open(root, true).await;
    put_and_flush(&store, 0..FRAGMENTS).await;
    assert!(
        journal_size(root) > 0,
        "the flush left the fragments in the journal, unfolded"
    );
}

fn journal_size(root: &Path) -> u64 {
    std::fs::metadata(root.join("immutable").join("journal")).map_or(0, |m| m.len())
}

async fn reads_back(store: &Arc<LocalImmutableStore>) -> u64 {
    reads_back_range(store, 0..FRAGMENTS).await
}

async fn reads_back_range(store: &Arc<LocalImmutableStore>, range: std::ops::Range<u64>) -> u64 {
    let mut found = 0;
    for n in range {
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
    store.fold_journal().await.expect("the fold succeeds");
    drop(store);

    let groups = bucket_files(&root);
    let written: Vec<_> = groups
        .iter()
        .filter(|(_, buckets, _)| *buckets > 0)
        .collect();
    assert!(!written.is_empty(), "the fold wrote the buckets");
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

/// Bucket files in the whole index.
fn bucket_count(root: &Path) -> usize {
    bucket_files(root)
        .iter()
        .map(|(_, buckets, _)| buckets)
        .sum()
}

/// Two journaled opens in a row with no fold between them: each replays the whole
/// journal, including what the open before it replayed, and appends after it. Reads see
/// every fragment either session stored, and a fold after the second writes them all.
#[tokio::test]
async fn opens_in_a_row_without_a_fold_read_back_everything() {
    let dir = lore_base::test_util::TempDir::new("lore-journal-reopen-twice-");
    let root = dir.path().join("store");
    write_and_close(&root).await;
    let first = journal_size(&root);

    let store = open(&root, true).await;
    assert_eq!(
        reads_back(&store).await,
        FRAGMENTS,
        "the first reopen replays"
    );
    put_and_flush(&store, FRAGMENTS..2 * FRAGMENTS).await;
    drop(store);
    assert!(
        journal_size(&root) > first,
        "the second session appended after the records the first left"
    );
    assert_eq!(bucket_count(&root), 0, "no fold ran between the opens");

    let store = open(&root, true).await;
    assert_eq!(
        reads_back_range(&store, 0..2 * FRAGMENTS).await,
        2 * FRAGMENTS,
        "the second reopen replays both sessions"
    );
    drop(store);

    // An open that changes nothing keeps the journal as it is, and the next reads too.
    let size = journal_size(&root);
    let store = open(&root, true).await;
    drop(store);
    assert_eq!(journal_size(&root), size, "an open alone appends nothing");

    let store = open(&root, true).await;
    store.fold_journal().await.expect("the fold succeeds");
    drop(store);
    assert_eq!(journal_size(&root), 0, "the fold empties the journal");
    assert!(bucket_count(&root) > 0, "the fold wrote the buckets");

    let store = open(&root, true).await;
    assert_eq!(
        reads_back_range(&store, 0..2 * FRAGMENTS).await,
        2 * FRAGMENTS,
        "everything reads back after the fold"
    );
}

/// Every file under `root`: its length and modification time.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)
            .expect("a directory reads")
            .flatten()
        {
            let metadata = entry.metadata().expect("metadata");
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                files.insert(
                    entry.path(),
                    (metadata.len(), metadata.modified().expect("mtime")),
                );
            }
        }
    }
    files
}

fn key(n: u64) -> Hash {
    lore_storage::hash_slice(format!("key {n}").as_bytes())
}

fn value(n: u64) -> Hash {
    lore_storage::hash_slice(format!("value {n}").as_bytes())
}

async fn open_mutable(root: &Path, immutable: &Arc<LocalImmutableStore>) -> LocalMutableStore {
    let immutable: Arc<dyn ImmutableStore> = immutable.clone();
    LocalMutableStore::new(
        Some(root),
        MutableStoreSettings {
            journal: true,
            ..Default::default()
        },
        immutable,
    )
    .await
    .expect("the mutable store opens")
}

/// A journaled open of stores whose journals hold records writes nothing: no packfile,
/// bucket, journal or info file changes length or modification time, through the open
/// and through reading everything back.
#[tokio::test]
async fn a_journaled_open_writes_nothing() {
    const KEYS: u64 = 200;
    let dir = lore_base::test_util::TempDir::new("lore-journal-reopen-no-writes-");
    let root = dir.path().join("store");
    {
        let immutable = open(&root, true).await;
        put_and_flush(&immutable, 0..FRAGMENTS).await;
        let mutable = Arc::new(open_mutable(&root, &immutable).await);
        for n in 0..KEYS {
            mutable
                .clone()
                .store(partition(), key(n), value(n), KeyType::Untyped)
                .await
                .expect("store succeeds");
        }
        mutable
            .clone()
            .flush(true)
            .await
            .expect("the flush succeeds");
    }
    assert!(
        journal_size(&root) > 0,
        "the immutable journal holds records"
    );
    assert!(
        std::fs::metadata(root.join("mutable").join("journal")).is_ok_and(|m| m.len() > 0),
        "the mutable journal holds records"
    );

    let before = snapshot(&root);
    // Coarse file systems stamp times to the second or worse; a write in this open would
    // land on a later stamp.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let immutable = open(&root, true).await;
    let mutable = Arc::new(open_mutable(&root, &immutable).await);
    assert_eq!(reads_back(&immutable).await, FRAGMENTS, "the open replays");
    for n in 0..KEYS {
        assert_eq!(
            mutable
                .clone()
                .load(partition(), key(n), KeyType::Untyped)
                .await
                .expect("a journaled key loads"),
            value(n)
        );
    }
    assert_eq!(
        snapshot(&root),
        before,
        "the open and the reads wrote nothing"
    );
}

/// A journal past the fold size at open is replayed like any other, and folded by the
/// first flush, before that flush appends. Records replay idempotently, so a journal that
/// holds its records many times over stands in for one a fold cut short left too large.
#[tokio::test]
async fn a_journal_past_the_fold_size_at_open_is_folded_by_the_first_flush() {
    let dir = lore_base::test_util::TempDir::new("lore-journal-reopen-oversize-");
    let root = dir.path().join("store");
    write_and_close(&root).await;
    let path = root.join("immutable").join("journal");
    let records = std::fs::read(&path).expect("the journal reads");
    let mut repeated = Vec::new();
    while (repeated.len() as u64) < JOURNAL_FOLD_SIZE {
        repeated.extend_from_slice(&records);
    }
    std::fs::write(&path, &repeated).expect("the journal writes");

    let store = open(&root, true).await;
    assert_eq!(reads_back(&store).await, FRAGMENTS, "the open replays");
    assert_eq!(
        journal_size(&root),
        repeated.len() as u64,
        "the open folds nothing"
    );
    assert_eq!(bucket_count(&root), 0, "the open writes no bucket");

    put_and_flush(&store, FRAGMENTS..FRAGMENTS + 1).await;
    assert_eq!(journal_size(&root), 0, "the first flush folded the journal");
    assert!(bucket_count(&root) > 0, "the fold wrote the buckets");
    drop(store);

    let store = open(&root, true).await;
    assert_eq!(
        reads_back_range(&store, 0..FRAGMENTS + 1).await,
        FRAGMENTS + 1,
        "everything reads back after the fold"
    );
}
