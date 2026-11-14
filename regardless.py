PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/immutable_store.rs'
EDITS = [
('''        if store.settings.journal
            && let Some(path) = immutable_path.as_deref()
        {
            let journal = store.open_journal(path).await?;
            store.journal = Some(Mutex::new(journal));
        }
''','''        // A journal a journaled open left behind is folded even by an open that keeps
        // none, so what it holds is not lost.
        if let Some(path) = immutable_path.as_deref()
            && (store.settings.journal || journal::holds_records(&path.join(JOURNAL_FILE)))
        {
            let journal = store.open_journal(path).await?;
            if store.settings.journal {
                store.journal = Some(Mutex::new(journal));
            } else {
                for group in &store.group {
                    group.packstore.set_deferred(false);
                }
            }
        }
'''),
]
