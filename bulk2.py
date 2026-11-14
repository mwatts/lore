PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/immutable_store.rs'
EDITS = [
('''/// Entry records past which a flush folds instead of journaling. A flush that changes
/// this many entries touches most buckets anyway, so journaling it only postpones the
/// same rewrites to a fold that also rewrites every bucket the journal touched before.
pub const JOURNAL_BULK_ENTRY_BYTES: usize = 256 * 1024;''','''/// Entry records past which a flush also writes its buckets, as well as journaling them.
/// A flush that changes this many entries (a revision of hundreds of objects) touches most
/// buckets anyway, so journaling it only postpones the same rewrites to a fold.
pub const JOURNAL_BULK_ENTRY_BYTES: usize = 64 * 1024;'''),
('''        if entry_records.len() >= JOURNAL_BULK_ENTRY_BYTES {
            // The fold writes every changed bucket as it is now, these entries included.
            return self.fold_locked(journal).await;
        }
        let mut records = Vec::new();''','''        // A bulk flush is journaled like any other, so a replay still ends at each entry's
        // newest value, and then also writes every changed bucket and syncs it, as a flush
        // without the journal does, instead of leaving those rewrites to the fold.
        let bulk = entry_records.len() >= JOURNAL_BULK_ENTRY_BYTES;
        let mut records = Vec::new();'''),
('''        for (group_index, id, offset) in journaled {
            self.group[group_index]
                .packstore
                .mark_journaled(id, offset)
                .await;
        }
        Ok(())
    }''','''        for (group_index, id, offset) in journaled {
            self.group[group_index]
                .packstore
                .mark_journaled(id, offset)
                .await;
        }
        if bulk && let Some(path) = self.path.as_ref() {
            if let Err(err) = flush_buckets(&self.group, path.clone(), sync_data).await {
                // The entries are journaled; a fold writes the buckets.
                self.journal_needs_fold
                    .store(true, atomic::Ordering::Relaxed);
                return Err(err);
            }
        }
        Ok(())
    }'''),
]
