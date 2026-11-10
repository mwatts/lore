// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
//! A repository written before node blocks shrank still reads, and commits.
//!
//! The fixture under `tests/data/legacy-node-blocks` was written by Lore
//! `4e9cdb9`, whose revision trees hold 512 nodes per block. Regenerate it with
//! a Lore at that revision by running the ignored `write_legacy_fixture` test
//! with `LORE_LEGACY_FIXTURE_OUT` naming an empty directory.
mod test_util;

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::Arc;

    use bytes::Bytes;
    use lore::file::LoreFileWriteArgs;
    use lore::repository::LoreRepositoryCreateArgs;
    use lore::revision::LoreRevisionCommitContentsArgs;
    use lore::revision::LoreRevisionInfoArgs;
    use lore_revision::interface::LoreEvent;
    use lore_revision::interface::LoreGlobalArgs;
    use lore_revision::interface::LoreString;
    use lore_revision::repository::LoreSharedStoreMode;
    use parking_lot::Mutex;

    use super::test_util::TempDir;

    /// Notes the first revision writes.
    const NOTES: usize = 700;
    /// Notes the second revision removes, leaving free slots in a block.
    const REMOVED: std::ops::Range<usize> = 100..160;
    /// Notes the second revision edits.
    const EDITED: std::ops::Range<usize> = 0..10;
    /// Files the third revision adds in a new directory.
    const OTHERS: usize = 5;

    struct Repository {
        out: TempDir,
        globals: LoreGlobalArgs,
    }

    impl Repository {
        fn at(path: &Path) -> Self {
            Self {
                out: TempDir::new("lore-legacy-blocks-out-"),
                globals: LoreGlobalArgs {
                    repository_path: path.into(),
                    offline: 1,
                    local: 1,
                    identity: "test-user".into(),
                    sync_data: 1,
                    ..Default::default()
                },
            }
        }

        fn path(&self) -> &Path {
            Path::new(self.globals.repository_path.as_str())
        }

        async fn commit(&self, writes: &[(String, Vec<u8>)], removals: &[String]) -> String {
            let revision: Arc<Mutex<Option<String>>> = Arc::default();
            let seen = revision.clone();
            let callback = Some(Box::new(move |event: &LoreEvent| {
                if let LoreEvent::RevisionCommitRevision(data) = event {
                    *seen.lock() = Some(data.revision.to_string());
                }
            }) as Box<dyn Fn(&LoreEvent) + Send + Sync>);
            let args = LoreRevisionCommitContentsArgs {
                writes: writes
                    .iter()
                    .map(|(path, bytes)| (path.clone(), Bytes::copy_from_slice(bytes)))
                    .collect(),
                removals: removals.to_vec(),
                message: "contents".into(),
            };
            let status =
                lore::revision::commit_contents(self.globals.clone(), args, callback).await;
            assert_eq!(status, 0, "the commit lands");
            let revision = revision.lock().clone();
            revision.expect("the commit reports its revision")
        }

        async fn current(&self) -> String {
            let revision: Arc<Mutex<String>> = Arc::default();
            let seen = revision.clone();
            let callback = Some(Box::new(move |event: &LoreEvent| {
                if let LoreEvent::RevisionInfo(data) = event {
                    *seen.lock() = data.revision.to_string();
                }
            }) as Box<dyn Fn(&LoreEvent) + Send + Sync>);
            let args = LoreRevisionInfoArgs {
                revision: LoreString::default(),
                delta: 0,
                metadata: 0,
            };
            assert_eq!(
                lore::revision::info(self.globals.clone(), args, callback).await,
                0
            );
            let revision = revision.lock().clone();
            revision
        }

        async fn read(&self, revision: &str, path: &str) -> Option<Vec<u8>> {
            static READS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let read = READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let output = self.out.path().join(format!("read-{read}"));
            let args = LoreFileWriteArgs {
                address: LoreString::default(),
                path: self.path().join(path).as_path().into(),
                revision: LoreString::from(revision),
                output: output.as_path().into(),
            };
            if lore::file::write(self.globals.clone(), args, None).await != 0 {
                return None;
            }
            Some(std::fs::read(&output).expect("written output reads back"))
        }

        async fn flush(&self) {
            assert_eq!(
                lore::repository::flush(
                    self.globals.clone(),
                    lore::repository::LoreRepositoryFlushArgs {},
                    None
                )
                .await,
                0,
                "the repository flushes"
            );
        }
    }

    fn note(n: usize, round: usize) -> Vec<u8> {
        format!("note {n} round {round}").into_bytes()
    }

    fn note_path(n: usize) -> String {
        format!("meta/notes/{n}")
    }

    fn other(k: usize) -> Vec<u8> {
        format!("other {k}").into_bytes()
    }

    fn other_path(k: usize) -> String {
        format!("meta/other/{k}")
    }

    /// What note `n` holds after the fixture's revision `revision` (0-based), if
    /// anything.
    fn note_at(revision: usize, n: usize) -> Option<Vec<u8>> {
        match revision {
            0 => Some(note(n, 1)),
            _ if REMOVED.contains(&n) => None,
            _ if EDITED.contains(&n) => Some(note(n, 2)),
            _ => Some(note(n, 1)),
        }
    }

    /// Write the fixture: three revisions spanning two 512-node blocks, the
    /// second of which frees slots in the first block.
    #[tokio::test]
    #[ignore = "writes the fixture; run with a Lore at 4e9cdb9"]
    async fn write_legacy_fixture() {
        let out = PathBuf::from(
            std::env::var_os("LORE_LEGACY_FIXTURE_OUT").expect("LORE_LEGACY_FIXTURE_OUT is set"),
        );
        let repository = Repository::at(&out);
        let args = LoreRepositoryCreateArgs {
            repository_url: "lore://localhost/legacy-node-blocks".into(),
            id: LoreString::default(),
            description: LoreString::default(),
            use_shared_store: LoreSharedStoreMode::Disabled,
            shared_store_path: LoreString::default(),
            vfs: Default::default(),
        };
        assert_eq!(
            lore::repository::create(repository.globals.clone(), args, None).await,
            0
        );
        let mut log = String::new();
        let first: Vec<(String, Vec<u8>)> =
            (0..NOTES).map(|n| (note_path(n), note(n, 1))).collect();
        log.push_str(&repository.commit(&first, &[]).await);
        log.push('\n');
        let edits: Vec<(String, Vec<u8>)> = EDITED.map(|n| (note_path(n), note(n, 2))).collect();
        let removals: Vec<String> = REMOVED.map(note_path).collect();
        log.push_str(&repository.commit(&edits, &removals).await);
        log.push('\n');
        let others: Vec<(String, Vec<u8>)> =
            (0..OTHERS).map(|k| (other_path(k), other(k))).collect();
        log.push_str(&repository.commit(&others, &[]).await);
        log.push('\n');
        repository.flush().await;
        std::fs::write(out.join("revisions.log"), log).expect("the log writes");
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("the copy's directory is made");
        for entry in std::fs::read_dir(from).expect("the fixture lists") {
            let entry = entry.expect("a fixture entry");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("a file type").is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).expect("a fixture file copies");
            }
        }
    }

    /// Check what every fixture revision holds, sampling the notes.
    async fn check_fixture_revisions(repository: &Repository, revisions: &[String]) {
        for (index, revision) in revisions.iter().enumerate() {
            for n in (0..NOTES).filter(|n| n % 7 == 0 || REMOVED.contains(n) || EDITED.contains(n))
            {
                assert_eq!(
                    repository.read(revision, &note_path(n)).await,
                    note_at(index, n),
                    "revision {index}, note {n}"
                );
            }
            for k in 0..OTHERS {
                let expected = (index >= 2).then(|| other(k));
                assert_eq!(
                    repository.read(revision, &other_path(k)).await,
                    expected,
                    "revision {index}, other {k}"
                );
            }
        }
    }

    /// Whether `revision`'s tree is held in 512-node blocks, and its node
    /// block count as this Lore counts blocks.
    async fn tree_blocks(path: &Path, revision: &str) -> (bool, u32) {
        use std::str::FromStr;
        let execution = Arc::new(lore_revision::interface::ExecutionContext::new_client(
            LoreGlobalArgs::default(),
            lore_revision::relay::EventDispatcher::no_dispatch(),
        ));
        let path = path.to_path_buf();
        let revision = lore_base::types::Hash::from_str(revision).expect("a revision hash");
        lore_base::runtime::LORE_CONTEXT
            .scope(execution, async move {
                let repository = lore_revision::repository::load_and_connect(
                    &path,
                    lore_revision::repository::RepositoryAccess::ReadOnly,
                )
                .await
                .expect("the repository opens");
                let state = lore_revision::state::State::deserialize(repository.clone(), revision)
                    .await
                    .expect("the revision reads");
                let legacy = state
                    .has_legacy_node_blocks(repository.clone())
                    .await
                    .expect("the tree reads");
                let tree = state.tree(repository).await.expect("the tree reads");
                (legacy, tree.block_count)
            })
            .await
    }

    /// A store Lore `4e9cdb9` wrote, with 512-node blocks, reads back at every
    /// revision, takes commits that add, edit, and remove, and still reads back
    /// every revision, old and new, after a reopen.
    #[tokio::test]
    async fn a_store_with_512_node_blocks_reads_and_commits() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/legacy-node-blocks");
        let dir = TempDir::new("lore-legacy-blocks-");
        copy_dir(&fixture, dir.path());
        let repository = Repository::at(dir.path());
        let revisions: Vec<String> = std::fs::read_to_string(dir.path().join("revisions.log"))
            .expect("the fixture logs its revisions")
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(revisions.len(), 3);
        assert_eq!(repository.current().await, revisions[2]);
        let split = lore_revision::node::LEGACY_BLOCK_SPLIT as u32;
        for revision in &revisions {
            assert_eq!(
                tree_blocks(dir.path(), revision).await,
                (true, 2 * split),
                "the fixture's trees are two 512-node blocks"
            );
        }
        check_fixture_revisions(&repository, &revisions).await;
        // The open stamped the store with a version Lore 4e9cdb9 refuses (it accepts 0 and
        // 1), so that binary cannot read the converted revisions below as empty.
        let info = std::fs::read(dir.path().join(".lore/immutable/info")).expect("info reads");
        assert_eq!(u32::from_ne_bytes(info[4..8].try_into().unwrap()), 2);

        // One edit, one add, one removal: the first commit on the old tree.
        let fourth = repository
            .commit(
                &[
                    (note_path(1), note(1, 4)),
                    ("meta/new/a".to_string(), b"new a".to_vec()),
                ],
                &[note_path(600)],
            )
            .await;
        // Enough adds to need nodes past every slot the old tree had free.
        let adds: Vec<(String, Vec<u8>)> = (0..500)
            .map(|n| (format!("meta/more/{n}"), note(n, 5)))
            .collect();
        let fifth = repository.commit(&adds, &[note_path(2)]).await;
        repository.flush().await;

        // A new process would see the same: nothing below is held from the commits.
        let reopened = Repository::at(dir.path());
        assert_eq!(reopened.current().await, fifth);
        check_fixture_revisions(&reopened, &revisions).await;
        // The first commit wrote the tree at the current block size, and the
        // revisions before it keep their 512-node blocks.
        for revision in &revisions {
            assert!(tree_blocks(dir.path(), revision).await.0);
        }
        assert_eq!(tree_blocks(dir.path(), &fourth).await, (false, 2 * split));
        let (legacy, fifth_blocks) = tree_blocks(dir.path(), &fifth).await;
        assert!(!legacy);
        assert!(
            fifth_blocks > 2 * split,
            "500 adds outgrow the slots the old tree had: {fifth_blocks} blocks"
        );
        assert_eq!(
            reopened.read(&fourth, &note_path(1)).await,
            Some(note(1, 4))
        );
        assert_eq!(
            reopened.read(&fourth, "meta/new/a").await.as_deref(),
            Some(&b"new a"[..])
        );
        assert_eq!(reopened.read(&fourth, &note_path(600)).await, None);
        assert_eq!(
            reopened.read(&fourth, &note_path(2)).await,
            Some(note(2, 2))
        );
        assert_eq!(reopened.read(&fifth, &note_path(2)).await, None);
        assert_eq!(reopened.read(&fifth, &note_path(1)).await, Some(note(1, 4)));
        for n in (0..NOTES).filter(|n| n % 7 == 3 && *n != 600 && *n != 2) {
            assert_eq!(
                reopened.read(&fifth, &note_path(n)).await,
                note_at(2, n),
                "note {n} at the tip"
            );
        }
        for n in [0, 1, 150, 299, 499] {
            assert_eq!(
                reopened.read(&fifth, &format!("meta/more/{n}")).await,
                Some(note(n, 5)),
                "added note {n}"
            );
        }
        for k in 0..OTHERS {
            assert_eq!(reopened.read(&fifth, &other_path(k)).await, Some(other(k)));
        }
        // Keep the converted store, for an older Lore to be tried on by hand.
        if let Some(keep) = std::env::var_os("LORE_LEGACY_KEEP") {
            std::fs::write(
                dir.path().join("revisions.log"),
                format!("{}\n{fourth}\n{fifth}\n", revisions.join("\n")),
            )
            .expect("the log writes");
            copy_dir(dir.path(), Path::new(&keep));
        }
    }
}
