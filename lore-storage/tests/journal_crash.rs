// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
//! A writer killed at any point leaves journaled stores that reopen with every
//! flush it finished, and nothing torn.
//!
//! The test runs itself as the writer: a child process stores rounds of
//! fragments and mutable entries in journaled stores on disk, flushes both with
//! `sync_data` after each round, and only then appends the round to a log,
//! synced. The parent kills it with SIGKILL partway, reopens the stores, and
//! checks that every logged round reads back whole, and that no fragment of a
//! later round reads back wrong. Rounds are large enough that some flushes fold
//! the journal, so kills land in folds too.

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

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

const CHILD: &str = "LORE_STORAGE_JOURNAL_CRASH_CHILD";
const FRAGMENTS_PER_ROUND: u64 = 24;

fn partition() -> Partition {
    Partition::from([7u8; 16])
}

/// The payload of fragment `n` of round `round`: under 1 KB to 200 KB, so a few
/// rounds pass the journal's fold size.
fn payload(round: u64, n: u64) -> Bytes {
    let len = 512 + ((round * 131 + n * 977) % 200) as usize * 1024;
    let seed = round * 1_000_003 + n;
    Bytes::from(
        (0..len)
            .map(|i| ((i as u64).wrapping_mul(2_654_435_761) ^ seed) as u8)
            .collect::<Vec<u8>>(),
    )
}

fn address(round: u64, n: u64) -> Address {
    Address {
        hash: lore_storage::hash_slice(&payload(round, n)),
        context: Context::default(),
    }
}

fn key(round: u64) -> Hash {
    lore_storage::hash_slice(format!("key {}", round % 5).as_bytes())
}

fn value(round: u64) -> Hash {
    lore_storage::hash_slice(format!("value {round}").as_bytes())
}

async fn open(root: &Path) -> (Arc<LocalImmutableStore>, Arc<LocalMutableStore>) {
    let immutable = LocalImmutableStore::new(
        Some(root.to_path_buf()),
        ImmutableStoreSettings {
            journal: true,
            ..Default::default()
        },
    )
    .await
    .expect("the immutable store opens");
    let dyn_immutable: Arc<dyn ImmutableStore> = immutable.clone();
    let mutable = Arc::new(
        LocalMutableStore::new(Some(root), MutableStoreSettings::default(), dyn_immutable)
            .await
            .expect("the mutable store opens"),
    );
    (immutable, mutable)
}

async fn write_round(
    immutable: &Arc<LocalImmutableStore>,
    mutable: &Arc<LocalMutableStore>,
    round: u64,
) {
    for n in 0..FRAGMENTS_PER_ROUND {
        let payload = payload(round, n);
        let fragment = Fragment {
            flags: 0,
            size_payload: payload.len() as u32,
            size_content: payload.len() as u64,
        };
        immutable
            .clone()
            .put(
                partition(),
                address(round, n),
                fragment,
                Some(payload),
                false,
            )
            .await
            .expect("put succeeds");
    }
    mutable
        .clone()
        .store(partition(), key(round), value(round), KeyType::Untyped)
        .await
        .expect("store succeeds");
    immutable
        .clone()
        .flush(true)
        .await
        .expect("the immutable flush succeeds");
    mutable
        .clone()
        .flush(true)
        .await
        .expect("the mutable flush succeeds");
}

fn child(root: PathBuf) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async move {
        let (immutable, mutable) = open(&root).await;
        let mut log = std::fs::OpenOptions::new()
            .append(true)
            .open(log_path(&root))
            .expect("the log opens");
        for round in 0.. {
            write_round(&immutable, &mutable, round).await;
            writeln!(log, "{round}").expect("log write");
            log.sync_all().expect("log sync");
        }
    });
}

fn log_path(root: &Path) -> PathBuf {
    root.with_extension("log")
}

fn logged(root: &Path) -> Vec<u64> {
    std::fs::read_to_string(log_path(root))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.parse().ok())
        .collect()
}

/// What `round`'s fragment `n` reads back as: `None` when the store has no entry.
async fn read(immutable: &Arc<LocalImmutableStore>, round: u64, n: u64) -> Option<Bytes> {
    let got = immutable
        .clone()
        .get(partition(), address(round, n))
        .await
        .ok()?;
    let payload = got.payload?;
    if got.fragment.flags & lore_storage::FragmentFlags::PayloadCompressed.bits() != 0 {
        let (_, content) = lore_storage::decompress(got.fragment, &payload).expect("decompresses");
        return Some(Bytes::from(content.to_vec()));
    }
    Some(payload)
}

#[test]
fn a_killed_writer_reopens_with_every_flush_it_finished() {
    if let Some(root) = std::env::var_os(CHILD) {
        child(PathBuf::from(root));
        return;
    }
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    for (attempt, extra_ms) in [0u64, 2, 5, 9, 14, 20, 27, 35, 44, 60]
        .into_iter()
        .enumerate()
    {
        let dir = lore_base::test_util::TempDir::new("lore-journal-crash-");
        let root = dir.path().join("store");
        std::fs::create_dir_all(&root).expect("store directory");
        std::fs::write(log_path(&root), b"").expect("log");
        let mut writer = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "a_killed_writer_reopens_with_every_flush_it_finished",
                "--test-threads=1",
            ])
            .env(CHILD, &root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the writer starts");
        let wanted = 2 + attempt;
        let started = Instant::now();
        while logged(&root).len() < wanted {
            assert!(
                started.elapsed() < Duration::from_secs(120),
                "attempt {attempt}: the writer logged {} rounds in 120 s",
                logged(&root).len()
            );
            assert!(
                writer.try_wait().expect("wait").is_none(),
                "attempt {attempt}: the writer exited"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        std::thread::sleep(Duration::from_millis(extra_ms));
        writer.kill().expect("kill");
        writer.wait().expect("reap");

        let rounds = logged(&root);
        let last = *rounds.last().expect("a logged round");
        runtime.block_on(async {
            let (immutable, mutable) = open(&root).await;
            for &round in &rounds {
                for n in 0..FRAGMENTS_PER_ROUND {
                    let got = read(&immutable, round, n).await;
                    assert!(
                        got.as_ref() == Some(&payload(round, n)),
                        "attempt {attempt}: round {round} fragment {n} was flushed and reads back {:?} bytes",
                        got.map(|bytes| bytes.len())
                    );
                }
            }
            // The round the kill interrupted may be partly there; what is there is whole.
            for n in 0..FRAGMENTS_PER_ROUND {
                if let Some(bytes) = read(&immutable, last + 1, n).await {
                    assert!(
                        bytes == payload(last + 1, n),
                        "attempt {attempt}: an unflushed fragment reads back torn"
                    );
                }
            }
            // Each key holds the value of the last round that wrote it, or a later one.
            for slot in 0..5u64 {
                let Some(written) = rounds.iter().rev().find(|round| *round % 5 == slot) else {
                    continue;
                };
                let got = mutable
                    .clone()
                    .load(partition(), key(*written), KeyType::Untyped)
                    .await
                    .expect("a flushed key loads");
                assert!(
                    got == value(*written) || got == value(written + 5),
                    "attempt {attempt}: key {slot} holds neither round {written} nor a later one"
                );
            }
            // The reopened stores take more writes, and keep them over another reopen.
            write_round(&immutable, &mutable, last + 2).await;
            drop(immutable);
            drop(mutable);
            let (immutable, _mutable) = open(&root).await;
            for n in 0..FRAGMENTS_PER_ROUND {
                assert!(
                    read(&immutable, last + 2, n).await == Some(payload(last + 2, n)),
                    "attempt {attempt}: a write after the reopen did not survive another reopen"
                );
            }
        });
    }
}
