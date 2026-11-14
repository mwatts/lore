PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/mutable_store.rs'
EDITS = [
('''        if settings.journal
            && let Some(path) = store.path.clone()
        {
            let journal = store.open_journal(path).await?;
            store.journal = Some(Mutex::new(journal));
        }
''','''        // A journal a journaled open left behind is folded even by an open that keeps
        // none, so what it holds is not lost.
        if let Some(path) = store.path.clone()
            && (settings.journal || journal::holds_records(&path.join(JOURNAL_FILE)))
        {
            let journal = store.open_journal(path).await?;
            if settings.journal {
                store.journal = Some(Mutex::new(journal));
            }
        }
'''),
]
