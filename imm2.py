PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/immutable_store.rs'
EDITS = [
('''pub const DEFAULT_FLUSH_DELAY_SECONDS: u64 = 5;
''','''pub const DEFAULT_FLUSH_DELAY_SECONDS: u64 = 5;

/// The store journal's file, beside `index/`.
pub const JOURNAL_FILE: &str = "journal";

/// Journal size at which a flush folds the journal into the packfiles. Payloads
/// wait in memory until then, so this bounds that memory too.
pub const JOURNAL_FOLD_SIZE: u64 = 16 * 1024 * 1024;
'''),
('''use crate::local::fan_out::GroupLevel;
''','''use crate::local::fan_out::GroupLevel;
use crate::local::journal;
use crate::local::journal::Journal;
use crate::local::journal::RecordKind;
'''),
('''    /// Per-bucket entry threshold that triggers fan-out at the next serialize. Default is `1000`.
    pub fan_out_threshold: usize,
}

impl Default for ImmutableStoreSettings {''','''    /// Per-bucket entry threshold that triggers fan-out at the next serialize. Default is `1000`.
    pub fan_out_threshold: usize,
    /// Make a flush durable through the store's journal (`local::journal`) for a store on
    /// disk: the payloads stored since the last flush are appended to the journal in one
    /// write and synced once, instead of syncing every packfile that took one. They stay in
    /// memory and reach their packfiles in a fold, when the journal passes
    /// [`JOURNAL_FOLD_SIZE`], before a compaction drops a packfile, and when the store opens.
    pub journal: bool,
}

impl Default for ImmutableStoreSettings {'''),
('''            initial_fan_out_level: 1,
            fan_out_threshold: crate::local::fan_out::FAN_OUT_THRESHOLD_DEFAULT,
        }
    }
}''','''            initial_fan_out_level: 1,
            fan_out_threshold: crate::local::fan_out::FAN_OUT_THRESHOLD_DEFAULT,
            journal: false,
        }
    }
}'''),
('''    #[cfg(feature = "failure_generator")]
    failure_generator: LocalImmutableStoreFailureGenerator,

    // This field must be dropped last so it must be declared last''','''    #[cfg(feature = "failure_generator")]
    failure_generator: LocalImmutableStoreFailureGenerator,

    /// The journal, when [`ImmutableStoreSettings::journal`] is set for a store on disk.
    /// Its lock also orders a flush against a fold.
    journal: Option<Mutex<Journal>>,

    // This field must be dropped last so it must be declared last'''),
('''            #[cfg(feature = "failure_generator")]
            failure_generator,
        };

        // With per group packstores''','''            #[cfg(feature = "failure_generator")]
            failure_generator,
            journal: None,
        };

        // With per group packstores'''),
('''        let store = Arc::new(store);

        // The `Arc` only exists now''','''        if store.settings.journal
            && let Some(path) = immutable_path.as_deref()
        {
            let journal = store.open_journal(path).await?;
            store.journal = Some(Mutex::new(journal));
        }

        let store = Arc::new(store);

        // The `Arc` only exists now'''),
('''    pub fn packstore(&self, group_index: usize) -> &crate::PackStore {
        &self.group[group_index].packstore
    }''','''    pub fn packstore(&self, group_index: usize) -> &crate::PackStore {
        &self.group[group_index].packstore
    }

    /// Open the journal, write what it holds back into the packfiles, sync them, and
    /// empty it; then defer payload writes to the journal from here on.
    ///
    /// Replay is idempotent: a payload record writes the same bytes at the same offset
    /// a fold already wrote, and a zero record zeroes them again. A compaction folds
    /// before it drops a packfile, so no record names a packfile reused since.
    async fn open_journal(&self, path: &Path) -> Result<Journal, LocalImmutableStoreError> {
        let (mut journal, records) = Journal::open(path.join(JOURNAL_FILE))
            .await
            .map_err(|err| {
                LocalImmutableStoreError::internal_with_context(err, "Failed to open the store journal")
            })?;
        let mut replayed: HashMap<usize, Vec<u32>> = HashMap::new();
        for record in &records {
            let (group, id, offset, bytes) = match record.kind {
                RecordKind::Payload => {
                    let Some((group, id, offset, bytes)) = journal::parse_payload(&record.body)
                    else {
                        continue;
                    };
                    (group, id, offset, bytes)
                }
                RecordKind::Zero => {
                    let Some((group, id, offset, len)) = journal::parse_zero(&record.body) else {
                        continue;
                    };
                    (group, id, offset, Bytes::from(vec![0u8; len as usize]))
                }
                _ => continue,
            };
            let Some(store_group) = self.group.get(group) else {
                continue;
            };
            store_group
                .packstore
                .replay_write(id, offset, bytes)
                .await
                .forward::<LocalImmutableStoreError>("Failed to replay the store journal")?;
            let ids = replayed.entry(group).or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        for (group, ids) in replayed {
            self.group[group]
                .packstore
                .sync_replayed(&ids)
                .await
                .forward::<LocalImmutableStoreError>("Failed to sync replayed packfiles")?;
        }
        journal.clear().await.map_err(|err| {
            LocalImmutableStoreError::internal_with_context(err, "Failed to empty the store journal")
        })?;
        for group in &self.group {
            group.packstore.set_deferred(true);
        }
        Ok(journal)
    }

    /// Append every payload stored since the last flush to the journal, in one write,
    /// synced once when `sync_data` is set.
    async fn journal_payloads(
        &self,
        journal: &mut Journal,
        sync_data: bool,
    ) -> Result<(), LocalImmutableStoreError> {
        let mut records = Vec::new();
        let mut journaled = Vec::new();
        for (group_index, group) in self.group.iter().enumerate() {
            for (id, offset, bytes) in group.packstore.unjournaled().await {
                journal::encode(
                    &mut records,
                    RecordKind::Payload,
                    &[&journal::payload_body(group_index as u32, id, offset), &bytes],
                );
                journaled.push((group_index, id, offset));
            }
        }
        journal.append(records, sync_data).await.map_err(|err| {
            LocalImmutableStoreError::internal_with_context(err, "Failed to append to the store journal")
        })?;
        for (group_index, id, offset) in journaled {
            self.group[group_index]
                .packstore
                .mark_journaled(id, offset)
                .await;
        }
        Ok(())
    }

    /// Write every payload waiting in memory to its packfile, sync them, and empty the
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
            LocalImmutableStoreError::internal_with_context(err, "Failed to empty the store journal")
        })
    }

    /// Fold the journal now, before an operation a replay must not undo, such as
    /// dropping a packfile a journal record names. A store without one has nothing to do.
    pub async fn fold_journal(&self) -> Result<(), LocalImmutableStoreError> {
        if let Some(journal) = self.journal.as_ref() {
            let mut journal = journal.lock().await;
            self.fold_payloads(&mut journal).await?;
        }
        Ok(())
    }

    /// Record in the journal that a payload range was zeroed, so a replay that writes
    /// the payload back zeroes it again.
    async fn journal_zero(
        &self,
        group_index: usize,
        id: u32,
        offset: u32,
        len: u32,
    ) -> Result<(), LocalImmutableStoreError> {
        let Some(journal) = self.journal.as_ref() else {
            return Ok(());
        };
        let mut records = Vec::new();
        journal::encode(
            &mut records,
            RecordKind::Zero,
            &[&journal::zero_body(group_index as u32, id, offset as u64, len as u64)],
        );
        journal.lock().await.append(records, true).await.map_err(|err| {
            LocalImmutableStoreError::internal_with_context(err, "Failed to append to the store journal")
        })
    }'''),
# flush_delayed: nothing to do for a journaled store
('''        tokio::time::sleep(Duration::from_secs(delay)).await;
        if let Some(store) = weak_ref.upgrade()
            && let Some(path) = store.path.as_ref()
        {
            let group = store.group[group_index].clone();

            for bucket_index in 0..group.bucket.len() {''','''        tokio::time::sleep(Duration::from_secs(delay)).await;
        if let Some(store) = weak_ref.upgrade()
            && let Some(path) = store.path.as_ref()
        {
            // A journaled store keeps payloads in memory until a fold, so a bucket written
            // here could reach the disk before the payloads it names. Its flushes write
            // the buckets after the journal holds those payloads.
            if store.journal.is_some() {
                return;
            }
            let group = store.group[group_index].clone();

            for bucket_index in 0..group.bucket.len() {'''),
# flush_all
('''        let Some(path) = path else {
            return Ok(());
        };

        let mut tasks = JoinSet::new();

        for group_index in 0..self.group.len() {
            let group = self.group[group_index].clone();

            // Lock-free scan: a group with no dirty bucket''','''        let Some(path) = path else {
            return Ok(());
        };

        // The journal holds the new payloads before any bucket naming them is written.
        let mut journal = match self.journal.as_ref() {
            Some(journal) => Some(journal.lock().await),
            None => None,
        };
        if let Some(journal) = journal.as_deref_mut() {
            self.journal_payloads(journal, sync_data).await?;
        }

        let mut tasks = JoinSet::new();

        for group_index in 0..self.group.len() {
            let group = self.group[group_index].clone();

            // Lock-free scan: a group with no dirty bucket'''),
('''        let mut result = Ok(());
        while let Some(task_result) = tasks.join_next().await {
            result = result.and(
                task_result
                    .internal("Task failed")
                    .map_err(LocalImmutableStoreError::from)
                    .flatten(),
            );
        }

        result?;
        Ok(())
    }''','''        let mut result = Ok(());
        while let Some(task_result) = tasks.join_next().await {
            result = result.and(
                task_result
                    .internal("Task failed")
                    .map_err(LocalImmutableStoreError::from)
                    .flatten(),
            );
        }

        result?;

        if let Some(journal) = journal.as_deref_mut()
            && journal.size() >= JOURNAL_FOLD_SIZE
        {
            self.fold_payloads(journal).await?;
        }
        Ok(())
    }'''),
# compaction: fold before truncating a packfile
('''            lore_base::lore_debug!(
                "Packstore compactor group {group_index} truncating packfile {packfile}"
            );
            let _ = group.packstore.truncate(packfile).await;''','''            // A journal record naming this packfile would write its old payloads over
            // whatever the packfile holds after it is reused, so empty the journal first.
            if let Err(err) = self.fold_journal().await {
                lore_base::lore_warn!(
                    "Packstore compactor could not fold the journal, keeping packfile {packfile}: {err}"
                );
                completed = false;
                break;
            }

            lore_base::lore_debug!(
                "Packstore compactor group {group_index} truncating packfile {packfile}"
            );
            let _ = group.packstore.truncate(packfile).await;'''),
]
