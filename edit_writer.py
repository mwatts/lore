p='/home/mwatts/code/github/mwatts/lore/lore/tests/revision_commit_contents.rs'
s=open(p).read()
start=s.index('    /// The writer: commit')
end=s.index('    /// Revisions committed past several journal folds')
new='''    /// The writer: commit `FOLD_REVISIONS` revisions, wait until they are durable,
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
            log.push('\\n');
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

'''
s=s[:start]+new+s[end:]
open(p,'w').write(s)
