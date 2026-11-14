PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/immutable_store.rs'
EDITS = [
('''pub const JOURNAL_FOLD_SIZE: u64 = 16 * 1024 * 1024;
''','''pub const JOURNAL_FOLD_SIZE: u64 = 16 * 1024 * 1024;

/// Entry records past which a flush folds instead of journaling. A flush that changes
/// this many entries touches most buckets anyway, so journaling it only postpones the
/// same rewrites to a fold that also rewrites every bucket the journal touched before.
pub const JOURNAL_BULK_ENTRY_BYTES: usize = 256 * 1024;
'''),
('''            taken.push((group_index, hashes));
        }
        let mut records = Vec::new();
        let mut journaled = Vec::new();''','''            taken.push((group_index, hashes));
        }
        if entry_records.len() >= JOURNAL_BULK_ENTRY_BYTES {
            // The fold writes every changed bucket as it is now, these entries included.
            return self.fold_locked(journal).await;
        }
        let mut records = Vec::new();
        let mut journaled = Vec::new();'''),
]
