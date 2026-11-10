// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
mod test_util;

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use bytes::Bytes;
    use lore::file::LoreFileStageArgs;
    use lore::file::LoreFileWriteArgs;
    use lore::repository::LoreRepositoryCreateArgs;
    use lore::revision::LoreRevisionCommitContentsArgs;
    use lore::revision::LoreRevisionInfoArgs;
    use lore_revision::interface::LoreArray;
    use lore_revision::interface::LoreEvent;
    use lore_revision::interface::LoreGlobalArgs;
    use lore_revision::interface::LoreString;
    use lore_revision::repository::LoreSharedStoreMode;
    use parking_lot::Mutex;

    use super::test_util::TempDir;

    struct Repository {
        _dir: Option<TempDir>,
        out: TempDir,
        globals: LoreGlobalArgs,
    }

    impl Repository {
        /// The repository another process created at `path`.
        fn at(path: &Path) -> Self {
            Self {
                _dir: None,
                out: TempDir::new("lore-commit-contents-out-"),
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

        async fn create(name: &str) -> Self {
            let dir = TempDir::new("lore-commit-contents-");
            let globals = LoreGlobalArgs {
                repository_path: dir.path().into(),
                offline: 1,
                local: 1,
                identity: "test-user".into(),
                sync_data: 1,
                ..Default::default()
            };
            let args = LoreRepositoryCreateArgs {
                repository_url: format!("lore://localhost/{name}").into(),
                id: LoreString::default(),
                description: LoreString::default(),
                use_shared_store: LoreSharedStoreMode::Disabled,
                shared_store_path: LoreString::default(),
                vfs: Default::default(),
            };
            assert_eq!(
                lore::repository::create(globals.clone(), args, None).await,
                0,
                "Failed to create repository"
            );
            Self {
                _dir: Some(dir),
                out: TempDir::new("lore-commit-contents-out-"),
                globals,
            }
        }

        fn path(&self) -> &Path {
            Path::new(self.globals.repository_path.as_str())
        }

        /// Commit `writes` and `removals`; the status and the revision the call
        /// reported, if it reported one.
        async fn commit(
            &self,
            writes: &[(&str, &[u8])],
            removals: &[&str],
        ) -> (i32, Option<String>) {
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
                    .map(|(path, bytes)| (path.to_string(), Bytes::copy_from_slice(bytes)))
                    .collect(),
                removals: removals.iter().map(|path| path.to_string()).collect(),
                message: "contents".into(),
            };
            let status =
                lore::revision::commit_contents(self.globals.clone(), args, callback).await;
            let revision = revision.lock().clone();
            (status, revision)
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

        /// The bytes `path` holds at `revision`, or `None` when it holds none.
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
    }

    #[tokio::test]
    async fn commits_files_without_touching_the_working_tree() {
        let repository = Repository::create("contents-commit").await;
        let large: Vec<u8> = (0u32..300_000).map(|n| (n * 7 % 251) as u8).collect();
        let (status, revision) = repository
            .commit(&[("meta/a", b"first"), ("bytes/large", &large)], &[])
            .await;
        assert_eq!(status, 0);
        let revision = revision.expect("the commit reports its revision");
        assert_eq!(
            repository.current().await,
            revision,
            "the current revision follows the commit"
        );
        assert_eq!(
            repository.read(&revision, "meta/a").await.as_deref(),
            Some(&b"first"[..])
        );
        assert_eq!(repository.read(&revision, "bytes/large").await, Some(large));
        assert!(
            !repository.path().join("meta").exists(),
            "nothing is written to the working tree"
        );

        let (status, second) = repository.commit(&[("meta/a", b"second")], &[]).await;
        assert_eq!(status, 0);
        let second = second.expect("an edit commits");
        assert_ne!(second, revision);
        assert_eq!(
            repository.read(&second, "meta/a").await.as_deref(),
            Some(&b"second"[..])
        );
        assert_eq!(
            repository.read(&revision, "meta/a").await.as_deref(),
            Some(&b"first"[..]),
            "the earlier revision keeps its bytes"
        );
    }

    #[tokio::test]
    async fn a_large_batch_adds_and_edits_in_one_directory() {
        let repository = Repository::create("contents-large-batch").await;
        let bytes = |n: usize, round: usize| format!("file {n} round {round}").into_bytes();
        let first: Vec<(String, Vec<u8>)> = (0..40)
            .map(|n| (format!("meta/notes/{n}"), bytes(n, 0)))
            .collect();
        let writes: Vec<(&str, &[u8])> = first
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .collect();
        let (status, revision) = repository.commit(&writes, &[]).await;
        assert_eq!(status, 0);
        let revision = revision.unwrap();
        // Half rewritten, half unchanged, and new names beside them.
        let second: Vec<(String, Vec<u8>)> = (0..60)
            .map(|n| (format!("meta/notes/{n}"), bytes(n, usize::from(n % 2 == 0))))
            .collect();
        let writes: Vec<(&str, &[u8])> = second
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .collect();
        let (status, next) = repository.commit(&writes, &[]).await;
        assert_eq!(status, 0);
        let next = next.unwrap();
        for (path, bytes) in &second {
            assert_eq!(
                repository.read(&next, path).await.as_deref(),
                Some(bytes.as_slice()),
                "{path}"
            );
        }
        for (path, bytes) in &first {
            assert_eq!(
                repository.read(&revision, path).await.as_deref(),
                Some(bytes.as_slice()),
                "{path}"
            );
        }
    }

    #[tokio::test]
    async fn unchanged_contents_commit_nothing() {
        let repository = Repository::create("contents-unchanged").await;
        let (_, first) = repository.commit(&[("meta/a", b"same")], &[]).await;
        let first = first.expect("the first commit lands");
        let (status, again) = repository
            .commit(&[("meta/a", b"same")], &["meta/absent"])
            .await;
        assert_eq!(status, 0, "nothing to commit is not a failure");
        assert_eq!(again, None, "no revision is reported");
        assert_eq!(repository.current().await, first);
    }

    #[tokio::test]
    async fn an_empty_file_commits_and_reads_back_empty() {
        let repository = Repository::create("contents-empty").await;
        let (status, first) = repository
            .commit(&[("meta/empty", b""), ("meta/full", b"bytes")], &[])
            .await;
        assert_eq!(status, 0, "an empty file commits");
        let first = first.expect("the first commit lands");
        assert_eq!(
            repository.read(&first, "meta/empty").await.as_deref(),
            Some(&b""[..])
        );
        // Filled, then emptied again: each is a change, and each reads back.
        let (_, filled) = repository.commit(&[("meta/empty", b"now full")], &[]).await;
        let filled = filled.expect("filling the file is a change");
        let (status, emptied) = repository.commit(&[("meta/empty", b"")], &[]).await;
        assert_eq!(status, 0);
        let emptied = emptied.expect("emptying the file is a change");
        assert_eq!(
            repository.read(&filled, "meta/empty").await.as_deref(),
            Some(&b"now full"[..])
        );
        assert_eq!(
            repository.read(&emptied, "meta/empty").await.as_deref(),
            Some(&b""[..])
        );
        let (_, again) = repository.commit(&[("meta/empty", b"")], &[]).await;
        assert_eq!(again, None, "an empty file left empty is no change");
    }

    #[tokio::test]
    async fn a_removal_leaves_the_tip_and_earlier_revisions_keep_it() {
        let repository = Repository::create("contents-remove").await;
        let (_, first) = repository
            .commit(&[("a/b/c", b"deep"), ("a/d", b"near")], &[])
            .await;
        let first = first.unwrap();
        let (status, second) = repository.commit(&[("a/e", b"new")], &["a/b/c"]).await;
        assert_eq!(status, 0);
        let second = second.unwrap();
        assert_eq!(repository.read(&second, "a/b/c").await, None);
        assert_eq!(
            repository.read(&second, "a/d").await.as_deref(),
            Some(&b"near"[..])
        );
        assert_eq!(
            repository.read(&second, "a/e").await.as_deref(),
            Some(&b"new"[..])
        );
        assert_eq!(
            repository.read(&first, "a/b/c").await.as_deref(),
            Some(&b"deep"[..])
        );
    }

    #[tokio::test]
    async fn a_write_under_a_file_publishes_nothing() {
        let repository = Repository::create("contents-under-file").await;
        let (_, first) = repository.commit(&[("f", b"a file")], &[]).await;
        let first = first.unwrap();
        let (status, revision) = repository
            .commit(&[("g", b"must not land"), ("f/x", b"under a file")], &[])
            .await;
        assert_ne!(status, 0);
        assert_eq!(revision, None);
        assert_eq!(repository.current().await, first);
        assert_eq!(repository.read(&first, "g").await, None);
        let (status, later) = repository.commit(&[("h", b"later")], &[]).await;
        assert_eq!(status, 0, "a refused commit leaves the repository writable");
        assert_eq!(repository.read(&later.unwrap(), "g").await, None);
    }

    #[tokio::test]
    async fn a_staged_state_is_not_published_past() {
        let repository = Repository::create("contents-staged").await;
        let (_, first) = repository.commit(&[("meta/a", b"one")], &[]).await;
        let first = first.unwrap();
        let staged = repository.path().join("staged.txt");
        std::fs::write(&staged, b"staged").unwrap();
        let args = LoreFileStageArgs {
            paths: LoreArray::from_vec(vec![LoreString::from(staged.as_path())]),
            case_change: 0,
            scan: 0,
        };
        assert_eq!(
            lore::file::stage(repository.globals.clone(), args, None).await,
            0
        );
        let (status, revision) = repository.commit(&[("meta/a", b"two")], &[]).await;
        assert_ne!(status, 0);
        assert_eq!(revision, None);
        assert_eq!(repository.current().await, first);
    }

    const FOLD_CHILD: &str = "LORE_COMMIT_CONTENTS_FOLD_CHILD";
    const FOLD_REVISIONS: u64 = 40;

    /// Revision `n`'s large file: 1 MiB that does not compress, so the revisions
    /// pass the immutable store journal's 16 MiB fold size.
    fn fold_payload(n: u64) -> Vec<u8> {
        let mut state = n.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        (0..1024 * 1024 / 8)
            .flat_map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state.to_le_bytes()
            })
            .collect()
    }

    fn fold_note(n: u64) -> Vec<u8> {
        format!("note {n}").into_bytes()
    }

    /// Bytes under `dir` in files whose path passes `keep`.
    fn bytes_under(dir: &Path, keep: &dyn Fn(&Path) -> bool) -> u64 {
        let mut total = 0;
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += bytes_under(&path, keep);
            } else if keep(&path) {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
        total
    }

    /// The writer: commit `FOLD_REVISIONS` revisions, wait until they are durable,
    /// log their ids, and end the process without closing anything, as a crash would.
    async fn fold_child(path: &Path) {
        let mut repository = Repository::at(path);
        let args = LoreRepositoryCreateArgs {
            repository_url: "lore://localhost/contents-folds".into(),
            id: LoreString::default(),
            description: LoreString::default(),
            use_shared_store: LoreSharedStoreMode::Disabled,
            shared_store_path: LoreString::default(),
            vfs: Default::default(),
        };
        assert_eq!(
            lore::repository::create(repository.globals.clone(), args, None).await,
            0,
            "the writer creates the repository"
        );
        // Keep the stores open from one call to the next, as a Cairn session does, so
        // the journal grows to its fold size instead of being folded by each open.
        repository.globals.store_keep_alive = 1;
        repository.globals.store_keep_alive_seconds = 2;
        let mut log = String::new();
        for n in 0..FOLD_REVISIONS {
            let large = fold_payload(n);
            let note = fold_note(n);
            let (status, revision) = repository
                .commit(
                    &[
                        (&format!("bytes/{n}"), &large),
                        (&format!("meta/notes/{n}"), &note),
                        ("meta/latest", &note),
                    ],
                    &[],
                )
                .await;
            assert_eq!(status, 0, "revision {n} commits");
            log.push_str(&revision.expect("a revision is reported"));
            log.push('\n');
        }
        // Wait for every call's flush, as Cairn does after a commit, so every revision
        // is durable before it is logged.
        assert_eq!(
            lore::repository::flush(
                repository.globals.clone(),
                lore::repository::LoreRepositoryFlushArgs {},
                None
            )
            .await,
            0,
            "the revisions flush"
        );
        std::fs::write(path.join("revisions.log"), log).expect("the log writes");
        std::process::exit(0);
    }

    /// Revisions committed across journal folds, by a process that ended
    /// without closing its stores, all read back whole in a new process: every
    /// file at every revision, with the bytes that revision gave it.
    #[tokio::test]
    async fn revisions_past_several_folds_reopen_whole() {
        if let Some(path) = std::env::var_os(FOLD_CHILD) {
            fold_child(Path::new(&path)).await;
            return;
        }
        // Only the writer opens the repository before the reopen below.
        let dir = TempDir::new("lore-commit-contents-folds-");
        let repository = Repository::at(dir.path());
        let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "tests::revisions_past_several_folds_reopen_whole",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(FOLD_CHILD, repository.path())
            .stdout(std::process::Stdio::null())
            .status()
            .expect("the writer runs");
        assert!(status.success(), "the writer committed every revision");
        let revisions: Vec<String> =
            std::fs::read_to_string(repository.path().join("revisions.log"))
                .expect("the writer logged its revisions")
                .lines()
                .map(str::to_owned)
                .collect();
        assert_eq!(revisions.len() as u64, FOLD_REVISIONS);

        // The premise: most payloads reached their packfiles before the reopen, through
        // a fold, and the journal still holds the revisions after it, for the reopen to
        // replay.
        let store = repository.path().join(".lore").join("immutable");
        let packed = bytes_under(&store, &|path| {
            path.components().any(|part| part.as_os_str() == "pack")
        });
        let journal = std::fs::metadata(store.join("journal"))
            .map(|m| m.len())
            .unwrap_or(0);
        assert!(
            packed >= 32 * 1024 * 1024,
            "the writer folded: {packed} B in packfiles, {journal} B in the journal"
        );
        assert!(
            journal > 0,
            "the writer left journaled revisions for the reopen to replay"
        );

        for (n, revision) in (0..FOLD_REVISIONS).zip(&revisions) {
            assert_eq!(
                repository.read(revision, &format!("bytes/{n}")).await,
                Some(fold_payload(n)),
                "revision {n}'s large file"
            );
            assert_eq!(
                repository.read(revision, "meta/latest").await,
                Some(fold_note(n)),
                "revision {n}'s edited file"
            );
            for earlier in [0, n / 2, n] {
                assert_eq!(
                    repository
                        .read(revision, &format!("meta/notes/{earlier}"))
                        .await,
                    Some(fold_note(earlier)),
                    "revision {n} holds note {earlier}"
                );
            }
        }
        assert_eq!(
            repository.current().await,
            *revisions.last().expect("a revision"),
            "the tip is the last revision the writer committed"
        );
    }
}
