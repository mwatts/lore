PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/immutable_store.rs'
EDITS = [
('''        if evict_count > 0 {
            lore_base::lore_debug!(
                "Evicted {evict_count} of {total_count} fragments from {bucket_count} buckets across {group_count} groups"
            );''','''        if evict_count > 0 {
            self.journal_unrecorded_change();
            lore_base::lore_debug!(
                "Evicted {evict_count} of {total_count} fragments from {bucket_count} buckets across {group_count} groups"
            );'''),
('''            if let Some(path) = path.as_ref() {
                let path = path.clone();
                let store = self.clone();
                lore_base::lore_spawn!(serialize_tasks, async move {''','''            // A journaled store writes buckets only in a fold, which writes the payloads
            // they name first; the next flush folds.
            self.journal_unrecorded_change();
            if let Some(path) = path.as_ref()
                && self.journal.is_none()
            {
                let path = path.clone();
                let store = self.clone();
                lore_base::lore_spawn!(serialize_tasks, async move {'''),
('''                if let Some(path) = path.as_ref() {
                    lore_base::lore_trace!(
                        "Packstore compactor serializing group {group_index} bucket {bucket_index}"
                    );''','''                // A journaled store writes buckets only in a fold, which writes the
                // payloads they name first; the fold before the truncate below does.
                self.journal_unrecorded_change();
                if let Some(path) = path.as_ref()
                    && self.journal.is_none()
                {
                    lore_base::lore_trace!(
                        "Packstore compactor serializing group {group_index} bucket {bucket_index}"
                    );'''),
]
