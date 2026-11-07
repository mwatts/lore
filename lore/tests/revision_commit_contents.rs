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
        _dir: TempDir,
        out: TempDir,
        globals: LoreGlobalArgs,
    }

    impl Repository {
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
                _dir: dir,
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
}
