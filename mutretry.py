PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/mutable_store.rs'
EDITS = [
('''        for group in &self.group {
            group.journal_pending.lock().clear();
        }
        flush_buckets(&self.group, path, true, self.authoritative).await?;
        journal.clear().await.map_err(|err| {
            LocalMutableStoreError::internal_with_context(err, "Failed to empty the store journal")
        })
    }''','''        self.journal_needs_fold
            .store(false, atomic::Ordering::Relaxed);
        for group in &self.group {
            group.journal_pending.lock().clear();
        }
        let folded = async {
            flush_buckets(&self.group, path, true, self.authoritative).await?;
            journal.clear().await.map_err(|err| {
                LocalMutableStoreError::internal_with_context(
                    err,
                    "Failed to empty the store journal",
                )
            })
        }
        .await;
        if folded.is_err() {
            // The pending lists are gone, so only a fold makes those changes durable:
            // the next flush tries again.
            self.journal_needs_fold
                .store(true, atomic::Ordering::Relaxed);
        }
        folded
    }'''),
('''            if journal.size() >= JOURNAL_FOLD_SIZE {
                return self.fold_locked(&mut journal, path).await;
            }''','''            if self.journal_needs_fold.load(atomic::Ordering::Relaxed)
                || journal.size() >= JOURNAL_FOLD_SIZE
            {
                return self.fold_locked(&mut journal, path).await;
            }'''),
('''    journal: Option<Mutex<Journal>>,

    // This field must be dropped last so it must be declared last''','''    journal: Option<Mutex<Journal>>,
    /// Set when a fold failed after taking the pending lists, so the next flush folds.
    journal_needs_fold: AtomicBool,

    // This field must be dropped last so it must be declared last'''),
('''            authoritative,
            journal: None,
        };''','''            authoritative,
            journal: None,
            journal_needs_fold: AtomicBool::new(false),
        };'''),
]
