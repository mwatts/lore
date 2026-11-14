PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/immutable_store.rs'
EDITS = [
# group: pending hashes
('''    pub packstore: crate::PackStore,
    pub flush: Mutex<JoinSet<()>>,
}

impl ImmutableStoreGroup {''','''    pub packstore: crate::PackStore,
    pub flush: Mutex<JoinSet<()>>,
    /// Hashes whose entries changed since the journal last took this group's changes. A
    /// journaled store's flush appends those entries to the journal instead of rewriting
    /// the buckets that hold them.
    pub journal_pending: parking_lot::Mutex<Vec<Hash>>,
}

impl ImmutableStoreGroup {'''),
('''                packstore: crate::PackStore::new(
                    packpath,
                    MIN_PACKFILE_COUNT,
                    Some(store.gc_counters.clone()),
                ),
                flush: Mutex::new(JoinSet::new()),
            }));''','''                packstore: crate::PackStore::new(
                    packpath,
                    MIN_PACKFILE_COUNT,
                    Some(store.gc_counters.clone()),
                ),
                flush: Mutex::new(JoinSet::new()),
                journal_pending: parking_lot::Mutex::new(Vec::new()),
            }));'''),
# store fields
('''    /// The journal, when [`ImmutableStoreSettings::journal`] is set for a store on disk.
    /// Its lock also orders a flush against a fold.
    journal: Option<Mutex<Journal>>,
''','''    /// The journal, when [`ImmutableStoreSettings::journal`] is set for a store on disk.
    /// Its lock also orders a flush against a fold.
    journal: Option<Mutex<Journal>>,
    /// Set by a change the journal does not record (an eviction, a compaction, an
    /// obliteration, a repair), so the next flush folds and the change is durable after it.
    journal_needs_fold: AtomicBool,
'''),
('''            failure_generator,
            journal: None,
        };''','''            failure_generator,
            journal: None,
            journal_needs_fold: AtomicBool::new(false),
        };'''),
# settings doc
('''    /// Make a flush durable through the store's journal (`local::journal`) for a store on
    /// disk: the payloads stored since the last flush are appended to the journal in one
    /// write and synced once, instead of syncing every packfile that took one. They stay in
    /// memory and reach their packfiles in a fold, when the journal passes
    /// [`JOURNAL_FOLD_SIZE`], before a compaction drops a packfile, and when the store opens.
    pub journal: bool,''','''    /// Make a flush durable through the store's journal (`local::journal`) for a store on
    /// disk: the payloads stored and the index entries changed since the last flush are
    /// appended to the journal in one write and synced once, instead of syncing every
    /// packfile that took a payload and rewriting every bucket that changed. They stay in
    /// memory and reach their packfiles and buckets in a fold: when the journal passes
    /// [`JOURNAL_FOLD_SIZE`], after a change the journal does not record (an eviction, a
    /// compaction, an obliteration), before a compaction drops a packfile, and when the
    /// store opens.
    pub journal: bool,'''),
# open_journal: entries + fold
('''    /// Open the journal, write what it holds back into the packfiles, sync them, and
    /// empty it; then defer payload writes to the journal from here on.
    ///
    /// Replay is idempotent: a payload record writes the same bytes at the same offset
    /// a fold already wrote, and a zero record zeroes them again. A compaction folds
    /// before it drops a packfile, so no record names a packfile reused since.
    async fn open_journal(&self, path: &Path) -> Result<Journal, LocalImmutableStoreError> {''','''    /// Open the journal and fold what it holds: payloads back into their packfiles,
    /// synced, entries into their buckets, written and synced, and then empty it. From
    /// here on payloads and entries go to the journal.
    ///
    /// Replay is idempotent: a payload record writes the same bytes at the same offset
    /// a fold already wrote, a zero record zeroes them again, and an entry record sets
    /// the entry to the value it had when the journal took it. The journal is emptied by
    /// every fold, and between folds no bucket is written, so no record is older than the
    /// bucket it lands in. A compaction folds before it drops a packfile, so no record
    /// names a packfile reused since.
    async fn open_journal(&self, path: &Path) -> Result<Journal, LocalImmutableStoreError> {'''),
('''        let mut replayed: HashMap<usize, Vec<u32>> = HashMap::new();
        for record in &records {
            let (group, id, offset, bytes) = match record.kind {''','''        let mut replayed: HashMap<usize, Vec<u32>> = HashMap::new();
        let mut entries = Vec::new();
        for record in &records {
            let (group, id, offset, bytes) = match record.kind {
                RecordKind::ImmutableEntry => {
                    if let Ok(entry) = ImmutableStoreEntry::read_from_bytes(&record.body) {
                        entries.push(entry);
                    }
                    continue;
                }'''),
('''        for (group, ids) in replayed {
            self.group[group]
                .packstore
                .sync_replayed(&ids)
                .await
                .forward::<LocalImmutableStoreError>("Failed to sync replayed packfiles")?;
        }
        journal.clear().await.map_err(|err| {''','''        for (group, ids) in replayed {
            self.group[group]
                .packstore
                .sync_replayed(&ids)
                .await
                .forward::<LocalImmutableStoreError>("Failed to sync replayed packfiles")?;
        }
        if !entries.is_empty() {
            for entry in entries {
                self.replay_entry(path, entry).await?;
            }
            flush_buckets(&self.group, Arc::new(path.to_path_buf()), true).await?;
        }
        journal.clear().await.map_err(|err| {'''),
# replace journal_payloads with journal_changes (payloads + entries); fold_payloads -> fold_locked
('''    /// Append every payload stored since the last flush to the journal, in one write,
    /// synced once when `sync_data` is set.
    async fn journal_payloads(
        &self,
        journal: &mut Journal,
        sync_data: bool,
    ) -> Result<(), LocalImmutableStoreError> {
        let mut records = Vec::new();
        let mut journaled = Vec::new();
        for (group_index, group) in self.group.iter().enumerate() {''','''    /// Set the entry a journal record holds in its bucket, at the bucket layout the
    /// store opened at, and mark the bucket for the fold that follows.
    async fn replay_entry(
        &self,
        path: &Path,
        entry: ImmutableStoreEntry,
    ) -> Result<(), LocalImmutableStoreError> {
        let group_index = entry.address.hash.data()[0] as usize;
        let group = &self.group[group_index];
        let count = group.bucket_count.load(atomic::Ordering::Relaxed);
        let bucket_index = crate::local::fan_out::bucket_index_for(&entry.address.hash, count);
        let mut bucket = group.bucket(bucket_index).clone().write_owned().await;
        if !bucket.deserialized {
            Box::pin(bucket.deserialize(
                &group.dirty[bucket_index],
                path,
                group_index,
                bucket_index,
                None,
            ))
            .await?;
        }
        let (match_slot, insert_slot, match_made) =
            Self::lookup(&bucket, entry.partition, entry.address, StoreMatch::MatchFull);
        if match_made == StoreMatch::MatchFull {
            let index = bucket.sorted_index[match_slot] as usize;
            bucket.entry[index].data = entry.data;
        } else {
            let count = bucket.entry.len();
            bucket.sorted_index.insert(insert_slot, count as u32);
            bucket.entry.push(entry);
        }
        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Record that the entries under `hash` in `group_index` changed, for the next flush
    /// to journal. A store without a journal writes their bucket instead.
    fn journal_entry_changed(&self, group_index: usize, hash: Hash) {
        if self.journal.is_some() {
            self.group[group_index].journal_pending.lock().push(hash);
        }
    }

    /// Note a change the journal does not record, so the next flush folds.
    fn journal_unrecorded_change(&self) {
        if self.journal.is_some() {
            self.journal_needs_fold
                .store(true, atomic::Ordering::Relaxed);
        }
    }

    /// Every entry of `group` under one of `hashes`, as they are now.
    async fn current_entries(
        &self,
        group_index: usize,
        hashes: &[Hash],
    ) -> Vec<ImmutableStoreEntry> {
        let group = &self.group[group_index];
        let mut by_bucket: HashMap<usize, Vec<Hash>> = HashMap::new();
        let count = group.bucket_count.load(atomic::Ordering::Relaxed);
        for hash in hashes {
            by_bucket
                .entry(crate::local::fan_out::bucket_index_for(hash, count))
                .or_default()
                .push(*hash);
        }
        let mut entries = Vec::new();
        for (bucket_index, mut wanted) in by_bucket {
            wanted.sort_unstable();
            wanted.dedup();
            let Some(bucket) = group.try_bucket(bucket_index) else {
                continue;
            };
            let bucket = bucket.read().await;
            for entry in bucket.entry.iter() {
                if wanted.binary_search(&entry.address.hash).is_ok() {
                    entries.push(*entry);
                }
            }
        }
        entries
    }

    /// Append every payload stored and every entry changed since the last flush to the
    /// journal, in one write, synced once when `sync_data` is set.
    ///
    /// Entries are read before payloads: a payload is stored before the entry naming it,
    /// so every entry journaled here has its payload journaled here or earlier.
    async fn journal_changes(
        &self,
        journal: &mut Journal,
        sync_data: bool,
    ) -> Result<(), LocalImmutableStoreError> {
        let mut entry_records = Vec::new();
        let mut taken: Vec<(usize, Vec<Hash>)> = Vec::new();
        for (group_index, group) in self.group.iter().enumerate() {
            let hashes = std::mem::take(&mut *group.journal_pending.lock());
            if hashes.is_empty() {
                continue;
            }
            for entry in self.current_entries(group_index, &hashes).await {
                journal::encode(&mut entry_records, RecordKind::ImmutableEntry, &[entry.as_bytes()]);
            }
            taken.push((group_index, hashes));
        }
        let mut records = Vec::new();
        let mut journaled = Vec::new();
        for (group_index, group) in self.group.iter().enumerate() {'''),
('''        journal.append(records, sync_data).await.map_err(|err| {
            LocalImmutableStoreError::internal_with_context(
                err,
                "Failed to append to the store journal",
            )
        })?;
        for (group_index, id, offset) in journaled {''','''        records.extend_from_slice(&entry_records);
        if let Err(err) = journal.append(records, sync_data).await {
            // Nothing was journaled: the next flush takes these entries again.
            for (group_index, hashes) in taken {
                self.group[group_index]
                    .journal_pending
                    .lock()
                    .extend(hashes);
            }
            return Err(LocalImmutableStoreError::internal_with_context(
                err,
                "Failed to append to the store journal",
            ));
        }
        for (group_index, id, offset) in journaled {'''),
('''    /// Write every payload waiting in memory to its packfile, sync them, and empty the
    /// journal, whose records are all durable in the packfiles now.
    async fn fold_payloads(&self, journal: &mut Journal) -> Result<(), LocalImmutableStoreError> {
        for group in &self.group {
            group
                .packstore
                .write_tails(true)
                .await
                .forward::<LocalImmutableStoreError>("Failed to fold the store journal")?;
        }
        journal.clear().await.map_err(|err| {
            LocalImmutableStoreError::internal_with_context(
                err,
                "Failed to empty the store journal",
            )
        })
    }''','''    /// Fold the journal: write every payload waiting in memory to its packfile and sync
    /// them, write every changed bucket and sync it, and empty the journal, whose records
    /// are all durable in the packfiles and buckets now.
    ///
    /// The pending entry lists are taken first: a change made after a bucket is written
    /// is journaled by the next flush, and one made before is in the bucket.
    async fn fold_locked(&self, journal: &mut Journal) -> Result<(), LocalImmutableStoreError> {
        self.journal_needs_fold
            .store(false, atomic::Ordering::Relaxed);
        for group in &self.group {
            group.journal_pending.lock().clear();
        }
        for group in &self.group {
            group
                .packstore
                .write_tails(true)
                .await
                .forward::<LocalImmutableStoreError>("Failed to fold the store journal")?;
        }
        if let Some(path) = self.path.as_ref() {
            flush_buckets(&self.group, path.clone(), true).await?;
        }
        journal.clear().await.map_err(|err| {
            LocalImmutableStoreError::internal_with_context(err, "Failed to empty the store journal")
        })
    }'''),
('''        if let Some(journal) = self.journal.as_ref() {
            let mut journal = journal.lock().await;
            self.fold_payloads(&mut journal).await?;
        }
        Ok(())
    }''','''        if let Some(journal) = self.journal.as_ref() {
            let mut journal = journal.lock().await;
            self.fold_locked(&mut journal).await?;
        }
        Ok(())
    }'''),
# flush_all: journal mode does not rewrite buckets
('''        // The journal holds the new payloads before any bucket naming them is written.
        let mut journal = match self.journal.as_ref() {
            Some(journal) => Some(journal.lock().await),
            None => None,
        };
        if let Some(journal) = journal.as_deref_mut() {
            self.journal_payloads(journal, sync_data).await?;
        }

        let mut tasks = JoinSet::new();
''','''        // A journaled store appends what changed and syncs once; its buckets and
        // packfiles catch up in a fold.
        if let Some(journal) = self.journal.as_ref() {
            let mut journal = journal.lock().await;
            if self.journal_needs_fold.load(atomic::Ordering::Relaxed)
                || journal.size() >= JOURNAL_FOLD_SIZE
            {
                return self.fold_locked(&mut journal).await;
            }
            self.journal_changes(&mut journal, sync_data).await?;
            if journal.size() >= JOURNAL_FOLD_SIZE {
                self.fold_locked(&mut journal).await?;
            }
            return Ok(());
        }

        flush_buckets(&self.group, path, sync_data).await
    }
}

/// Write every dirty bucket of every group, one flusher per group at a time.
async fn flush_buckets(
    groups: &[Arc<ImmutableStoreGroup>],
    path: Arc<PathBuf>,
    sync_data: bool,
) -> Result<(), LocalImmutableStoreError> {
    {
        let mut tasks = JoinSet::new();
'''),
('''        for group_index in 0..self.group.len() {
            let group = self.group[group_index].clone();

            // Lock-free scan: a group with no dirty bucket''','''        for (group_index, group) in groups.iter().enumerate() {
            let group = group.clone();

            // Lock-free scan: a group with no dirty bucket'''),
('''        result?;

        if let Some(journal) = journal.as_deref_mut()
            && journal.size() >= JOURNAL_FOLD_SIZE
        {
            self.fold_payloads(journal).await?;
        }
        Ok(())
    }''','''        result?;
        Ok(())
    }
}

impl LocalImmutableStore {'''),
# store(): record the changed hash
('''        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
        drop(bucket);

        {
            let mut flush = group.flush.lock().await;
            let _ = flush.try_join_next();

            let stored_durable''','''        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
        drop(bucket);
        self.journal_entry_changed(group_index, address.hash);

        {
            let mut flush = group.flush.lock().await;
            let _ = flush.try_join_next();

            let stored_durable'''),
('''                        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);

                        lore_base::lore_trace!(
                            "Immutable store updated flags for {} from {} to {}",''','''                        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
                        self.journal_entry_changed(group_index, address.hash);

                        lore_base::lore_trace!(
                            "Immutable store updated flags for {} from {} to {}",'''),
# obliterate
('''        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
        drop(bucket);

        // The journal may hold the payload just zeroed''','''        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
        drop(bucket);
        self.journal_unrecorded_change();

        // The journal may hold the payload just zeroed'''),
]
