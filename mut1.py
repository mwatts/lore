PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/mutable_store.rs'
EDITS = [
('''use crate::local::fan_out::GroupLevel;
''','''use crate::local::fan_out::GroupLevel;
use crate::local::journal;
use crate::local::journal::Journal;
use crate::local::journal::RecordKind;
'''),
('''pub const DEFAULT_FLUSH_DELAY_SECONDS: u64 = 0;
''','''pub const DEFAULT_FLUSH_DELAY_SECONDS: u64 = 0;

/// The store journal's file, beside `index/`.
pub const JOURNAL_FILE: &str = "journal";

/// Journal size at which a flush folds the journal into the buckets.
pub const JOURNAL_FOLD_SIZE: u64 = 4 * 1024 * 1024;
'''),
('''    /// Source of truth (server) rather than a cache. When `true`, a corrupt bucket is a hard
    /// error; when `false`, it is reset to empty since its entries repopulate on next sync.
    pub authoritative: bool,
}''','''    /// Source of truth (server) rather than a cache. When `true`, a corrupt bucket is a hard
    /// error; when `false`, it is reset to empty since its entries repopulate on next sync.
    pub authoritative: bool,
    /// Make a flush durable through the store's journal (`local::journal`) for a store on
    /// disk: the entries changed since the last flush are appended to the journal in one
    /// write and synced once, instead of rewriting every bucket that changed. The buckets
    /// catch up in a fold, when the journal passes [`JOURNAL_FOLD_SIZE`] and when the
    /// store opens.
    pub journal: bool,
}'''),
('''            fan_out_threshold: crate::local::fan_out::FAN_OUT_THRESHOLD_DEFAULT,
            authoritative: false,
        }
    }
}''','''            fan_out_threshold: crate::local::fan_out::FAN_OUT_THRESHOLD_DEFAULT,
            authoritative: false,
            journal: false,
        }
    }
}'''),
('''    pub flush_lock: Arc<Mutex<()>>,
}

impl MutableStoreGroup {''','''    pub flush_lock: Arc<Mutex<()>>,
    /// Keys whose entries changed since the journal last took this group's changes. A
    /// journaled store's flush appends those entries instead of rewriting their buckets.
    pub journal_pending: parking_lot::Mutex<Vec<(Partition, Hash)>>,
}

impl MutableStoreGroup {'''),
('''    pub needs_upgrade: AtomicBool,
    pub authoritative: bool,

    // This field must be dropped last so it must be declared last''','''    pub needs_upgrade: AtomicBool,
    pub authoritative: bool,
    /// The journal, when [`MutableStoreSettings::journal`] is set for a store on disk. Its
    /// lock also orders a flush against a fold.
    journal: Option<Mutex<Journal>>,

    // This field must be dropped last so it must be declared last'''),
('''            needs_upgrade: AtomicBool::new(needs_upgrade),
            authoritative,
        };''','''            needs_upgrade: AtomicBool::new(needs_upgrade),
            authoritative,
            journal: None,
        };'''),
('''                committed_level: std::sync::atomic::AtomicUsize::new(committed),
                flush_lock: Arc::new(Mutex::new(())),
            }));
        }

        Ok(store)
    }''','''                committed_level: std::sync::atomic::AtomicUsize::new(committed),
                flush_lock: Arc::new(Mutex::new(())),
                journal_pending: parking_lot::Mutex::new(Vec::new()),
            }));
        }

        if settings.journal
            && let Some(path) = store.path.clone()
        {
            let journal = store.open_journal(path).await?;
            store.journal = Some(Mutex::new(journal));
        }

        Ok(store)
    }

    /// Open the journal, fold the entries it holds into their buckets, write and sync
    /// them, and empty it. Replay is idempotent: each record sets its entry to the value
    /// it had when the journal took it, in order, and since every fold empties the
    /// journal and no bucket is written between folds, no record is older than the
    /// bucket it lands in.
    async fn open_journal(&self, path: Arc<PathBuf>) -> Result<Journal, LocalMutableStoreError> {
        let (mut journal, records) = Journal::open(path.join(JOURNAL_FILE))
            .await
            .map_err(|err| {
                LocalMutableStoreError::internal_with_context(err, "Failed to open the store journal")
            })?;
        let mut replayed = false;
        for record in &records {
            if record.kind != RecordKind::MutableEntry {
                continue;
            }
            let Ok(entry) = MutableStoreEntry::read_from_bytes(&record.body) else {
                continue;
            };
            self.replay_entry(path.as_ref(), entry).await?;
            replayed = true;
        }
        if replayed {
            flush_buckets(&self.group, path.clone(), true, self.authoritative).await?;
        }
        journal.clear().await.map_err(|err| {
            LocalMutableStoreError::internal_with_context(err, "Failed to empty the store journal")
        })?;
        Ok(journal)
    }

    /// Set the entry a journal record holds in its bucket, at the layout the store
    /// opened at, and mark the bucket for the fold that follows.
    async fn replay_entry(
        &self,
        path: &Path,
        entry: MutableStoreEntry,
    ) -> Result<(), LocalMutableStoreError> {
        let group_index = entry.key.data()[0] as usize;
        let group = &self.group[group_index];
        let count = group.bucket_count.load(atomic::Ordering::Relaxed);
        let bucket_index = crate::local::fan_out::bucket_index_for(&entry.key, count);
        let mut bucket = group.bucket(bucket_index).write().await;
        if !bucket.deserialized {
            Box::pin(bucket.deserialize(
                path,
                group_index,
                bucket_index,
                false,
                self.authoritative,
            ))
            .await?;
        }
        let (_, match_made, insert_slot) = bucket.lookup(entry.partition, entry.key);
        if match_made {
            let index = bucket.sorted_index[insert_slot] as usize;
            bucket.entry[index].value = entry.value;
        } else if !entry.value.is_zero() {
            let count = bucket.entry.len();
            bucket.sorted_index.insert(insert_slot, count as u32);
            bucket.entry.push(entry);
        }
        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Record that the entry under `key` changed, for the next flush to journal.
    fn journal_entry_changed(&self, group_index: usize, partition: Partition, key: Hash) {
        if self.journal.is_some() {
            self.group[group_index]
                .journal_pending
                .lock()
                .push((partition, key));
        }
    }

    /// Append every entry changed since the last flush to the journal, in one write,
    /// synced once when `sync_data` is set.
    async fn journal_changes(
        &self,
        journal: &mut Journal,
        sync_data: bool,
    ) -> Result<(), LocalMutableStoreError> {
        let mut records = Vec::new();
        let mut taken: Vec<(usize, Vec<(Partition, Hash)>)> = Vec::new();
        for (group_index, group) in self.group.iter().enumerate() {
            let mut keys = std::mem::take(&mut *group.journal_pending.lock());
            if keys.is_empty() {
                continue;
            }
            keys.sort_unstable();
            keys.dedup();
            let count = group.bucket_count.load(atomic::Ordering::Relaxed);
            for (partition, key) in &keys {
                let bucket_index = crate::local::fan_out::bucket_index_for(key, count);
                let Some(bucket) = group.try_bucket(bucket_index) else {
                    continue;
                };
                let bucket = bucket.read().await;
                let (value, found, _) = bucket.lookup(*partition, *key);
                if !found {
                    continue;
                }
                let entry = MutableStoreEntry {
                    partition: *partition,
                    key: *key,
                    value,
                };
                journal::encode(&mut records, RecordKind::MutableEntry, &[entry.as_bytes()]);
            }
            taken.push((group_index, keys));
        }
        if let Err(err) = journal.append(records, sync_data).await {
            // Nothing was journaled: the next flush takes these entries again.
            for (group_index, keys) in taken {
                self.group[group_index].journal_pending.lock().extend(keys);
            }
            return Err(LocalMutableStoreError::internal_with_context(
                err,
                "Failed to append to the store journal",
            ));
        }
        Ok(())
    }

    /// Fold the journal: write every changed bucket, sync it, and empty the journal. The
    /// pending lists are taken first: a change made after a bucket is written is
    /// journaled by the next flush, and one made before is in the bucket.
    async fn fold_locked(
        &self,
        journal: &mut Journal,
        path: Arc<PathBuf>,
    ) -> Result<(), LocalMutableStoreError> {
        for group in &self.group {
            group.journal_pending.lock().clear();
        }
        flush_buckets(&self.group, path, true, self.authoritative).await?;
        journal.clear().await.map_err(|err| {
            LocalMutableStoreError::internal_with_context(err, "Failed to empty the store journal")
        })
    }'''),
('''        let Some(path) = path else {
            return Ok(());
        };
        let path = Arc::new(path.as_ref().clone());

        let mut tasks = JoinSet::new();
        let authoritative = self.authoritative;

        for (group_index, group) in self.group.iter().enumerate() {''','''        let Some(path) = path else {
            return Ok(());
        };
        let path = Arc::new(path.as_ref().clone());

        // A journaled store appends what changed and syncs once; its buckets catch up
        // in a fold.
        if let Some(journal) = self.journal.as_ref() {
            let mut journal = journal.lock().await;
            if journal.size() >= JOURNAL_FOLD_SIZE {
                return self.fold_locked(&mut journal, path).await;
            }
            return self.journal_changes(&mut journal, sync_data).await;
        }

        flush_buckets(&self.group, path, sync_data, self.authoritative).await
    }
}

/// Write every dirty bucket of every group, one flusher per group at a time.
async fn flush_buckets(
    groups: &[Arc<MutableStoreGroup>],
    path: Arc<PathBuf>,
    sync_data: bool,
    authoritative: bool,
) -> Result<(), LocalMutableStoreError> {
    {
        let mut tasks = JoinSet::new();

        for (group_index, group) in groups.iter().enumerate() {'''),
('''            bucket.entry.push(MutableStoreEntry {
                key: key.hash(),
                partition,
                value,
            });
        }

        self.clone()
            .mark_dirty(&mut bucket, group_index, bucket_index);

        Ok(())
    }''','''            bucket.entry.push(MutableStoreEntry {
                key: key.hash(),
                partition,
                value,
            });
        }

        self.clone()
            .mark_dirty(&mut bucket, group_index, bucket_index);
        self.journal_entry_changed(group_index, partition, key.hash());

        Ok(())
    }'''),
('''        self.clone()
            .mark_dirty(&mut bucket, group_index, bucket_index);

        // Value was created or updated, return previously stored value (the expected) to indicate this''','''        self.clone()
            .mark_dirty(&mut bucket, group_index, bucket_index);
        self.journal_entry_changed(group_index, partition, key.hash());

        // Value was created or updated, return previously stored value (the expected) to indicate this'''),
]
