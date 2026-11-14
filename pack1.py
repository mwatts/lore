PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/packstore.rs'
EDITS = [
('''struct PackFile {
    id: u32,
    size: usize,
    file: Option<IoFile>,
    buffer: Vec<u8>,
    dirty: AtomicBool,
}
''','''struct PackFile {
    id: u32,
    size: usize,
    file: Option<IoFile>,
    buffer: Vec<u8>,
    dirty: AtomicBool,
    /// Payloads stored but not yet written to `file`, in offset order, ending at
    /// `size`. Only a deferred store keeps any: its payloads are made durable by
    /// the store's journal, and reach the file in a fold
    /// ([`PackStore::write_tails`]).
    tail: Vec<TailItem>,
}

struct TailItem {
    offset: usize,
    bytes: Bytes,
    /// Whether the store's journal holds this payload yet.
    journaled: bool,
}

impl PackFile {
    /// The bytes `[offset, offset + size)` if they lie in one payload of the tail.
    fn tail_slice(&self, offset: usize, size: usize) -> Option<Bytes> {
        let first = self.tail.first()?;
        if offset < first.offset {
            return None;
        }
        let index = match self.tail.binary_search_by_key(&offset, |item| item.offset) {
            Ok(index) => index,
            Err(0) => return None,
            Err(index) => index - 1,
        };
        let item = &self.tail[index];
        let start = offset - item.offset;
        let end = start.checked_add(size)?;
        (end <= item.bytes.len()).then(|| item.bytes.slice(start..end))
    }

    /// Write the whole tail to the file at its offset, in one write, and sync it.
    async fn write_tail(&mut self, sync_data: bool) -> Result<(), PackfileError> {
        if self.tail.is_empty() {
            return Ok(());
        }
        let Some(file) = self.file.as_ref() else {
            self.tail.clear();
            return Ok(());
        };
        let start = self.tail[0].offset;
        let mut buffer = BytesMut::with_capacity(self.size - start);
        for item in &self.tail {
            debug_assert_eq!(item.offset, start + buffer.len(), "a tail is contiguous");
            buffer.extend_from_slice(&item.bytes);
        }
        packfile_write(file, buffer.freeze(), start).await?;
        if sync_data {
            crate::fs_util::retry_transient(|| file.sync_data())
                .await
                .internal("Failed syncing packstore file")?;
        }
        self.tail.clear();
        self.dirty.store(false, Ordering::Relaxed);
        Ok(())
    }
}
'''),
('''    gc_counters: Option<Arc<crate::maintenance::GcCounters>>,
}

impl PackStore {''','''    gc_counters: Option<Arc<crate::maintenance::GcCounters>>,
    /// Keep stored payloads in memory until a fold instead of writing each to its
    /// packfile as it is stored. Set by a store whose journal makes them durable.
    deferred: AtomicBool,
}

impl PackStore {'''),
('''            writeable: RwLock::default(),
            gc_counters,
        }
    }''','''            writeable: RwLock::default(),
            gc_counters,
            deferred: AtomicBool::new(false),
        }
    }

    /// Defer payload writes to [`Self::write_tails`]; see the `deferred` field.
    pub fn set_deferred(&self, deferred: bool) {
        self.deferred.store(deferred, Ordering::Relaxed);
    }'''),
('''                    buffer: vec![],
                    dirty: AtomicBool::new(false),
''','''                    buffer: vec![],
                    dirty: AtomicBool::new(false),
                    tail: Vec::new(),
''', 0),
('''            packfile.buffer.clear();
            packfile.size = 0;''','''            packfile.buffer.clear();
            packfile.tail.clear();
            packfile.size = 0;'''),
('''        // We got lock on the right packfile, load data
        if let Some(file) = packfile.file.as_ref() {
            return packfile_read(file, offset, size).await;
        }''','''        if let Some(bytes) = packfile.tail_slice(offset, size) {
            return Ok(bytes);
        }

        // We got lock on the right packfile, load data
        if let Some(file) = packfile.file.as_ref() {
            return packfile_read(file, offset, size).await;
        }'''),
('''        if let Some(file) = packfile.file.as_ref() {
            return packfile_read_into(file, offset, dst, size).await;
        }''','''        if let Some(bytes) = packfile.tail_slice(offset, size) {
            let Some(target) = dst.as_mut_slice().get_mut(..size) else {
                return Err(PackfileError::internal(
                    "Destination buffer is smaller than the payload to read",
                ));
            };
            target.copy_from_slice(&bytes);
            return Ok(());
        }

        if let Some(file) = packfile.file.as_ref() {
            return packfile_read_into(file, offset, dst, size).await;
        }'''),
('''        packfile.dirty.store(true, Ordering::Relaxed);

        if let Some(file) = packfile.file.as_ref() {
            packfile_write(file, buffer, offset).await?;
            packfile.size += size;''','''        if packfile.file.is_some() && self.deferred.load(Ordering::Relaxed) {
            packfile.tail.push(TailItem {
                offset,
                bytes: buffer,
                journaled: false,
            });
            packfile.size += size;
            if packfile.size >= PACKSTORE_SIZE_LIMIT as usize {
                full = true;
            }
        } else if let Some(file) = packfile.file.as_ref() {
            packfile.dirty.store(true, Ordering::Relaxed);
            packfile_write(file, buffer, offset).await?;
            packfile.size += size;'''),
('''        } else {
            if (packfile.buffer.len() + size) > packfile.buffer.capacity() {''','''        } else {
            packfile.dirty.store(true, Ordering::Relaxed);
            if (packfile.buffer.len() + size) > packfile.buffer.capacity() {'''),
('''        packfile.dirty.store(true, Ordering::Relaxed);

        if let Some(file) = packfile.file.as_ref() {
            packfile_write(file, zeros, offset).await?;
            let _ = file.sync_data().await;
        }

        Ok(())
    }''','''        let mut packfile = packfile;
        if let Some(item) = packfile
            .tail
            .iter_mut()
            .find(|item| item.offset <= offset && offset + size <= item.offset + item.bytes.len())
        {
            let start = offset - item.offset;
            let mut bytes = BytesMut::from(item.bytes.as_ref());
            bytes[start..start + size].fill(0);
            item.bytes = bytes.freeze();
            return Ok(());
        }

        packfile.dirty.store(true, Ordering::Relaxed);

        if let Some(file) = packfile.file.as_ref() {
            packfile_write(file, zeros, offset).await?;
            let _ = file.sync_data().await;
        }

        Ok(())
    }'''),
('''        let index = (id - 1) as usize;
        let packfile = packfile[index].read().await;
        if packfile.id != id {
            return Err(PackfileError::internal("Packfile ID mismatch index"));
        }

        if packfile
            .dirty''','''        let index = (id - 1) as usize;
        let mut packfile = packfile[index].write().await;
        if packfile.id != id {
            return Err(PackfileError::internal("Packfile ID mismatch index"));
        }

        if !packfile.tail.is_empty() {
            return packfile.write_tail(sync_data).await;
        }

        if packfile
            .dirty'''),
('''    pub async fn total_size(&self) -> Result<usize, PackfileError> {''','''    /// Every payload of a tail the journal does not hold yet: packfile id, offset,
    /// bytes. The caller journals them, then [`Self::mark_journaled`].
    pub async fn unjournaled(&self) -> Vec<(u32, u64, Bytes)> {
        let packfiles = self.packfile.read().await;
        let mut out = Vec::new();
        for packfile in packfiles.iter() {
            let packfile = packfile.read().await;
            for item in packfile.tail.iter().filter(|item| !item.journaled) {
                out.push((packfile.id, item.offset as u64, item.bytes.clone()));
            }
        }
        out
    }

    /// Record that the journal holds the payload at `offset` of packfile `id`.
    /// A payload no longer in a tail reached its file meanwhile, and is skipped.
    pub async fn mark_journaled(&self, id: u32, offset: u64) {
        let packfiles = self.packfile.read().await;
        let Some(packfile) = (id as usize)
            .checked_sub(1)
            .and_then(|index| packfiles.get(index))
        else {
            return;
        };
        let mut packfile = packfile.write().await;
        if let Ok(index) = packfile
            .tail
            .binary_search_by_key(&(offset as usize), |item| item.offset)
        {
            packfile.tail[index].journaled = true;
        }
    }

    /// Write every tail to its packfile and sync it: the fold of a deferred store.
    pub async fn write_tails(&self, sync_data: bool) -> Result<(), PackfileError> {
        let packfiles = self.packfile.read().await;
        for packfile in packfiles.iter() {
            packfile.write().await.write_tail(sync_data).await?;
        }
        Ok(())
    }

    /// Bytes held in tails, not yet written to packfiles.
    pub async fn tail_size(&self) -> usize {
        let packfiles = self.packfile.read().await;
        let mut size = 0;
        for packfile in packfiles.iter() {
            size += packfile
                .read()
                .await
                .tail
                .iter()
                .map(|item| item.bytes.len())
                .sum::<usize>();
        }
        size
    }

    /// Write journaled bytes back to packfile `id` at `offset`, before the store
    /// resumes: replaying a journal after a crash. Idempotent. The caller syncs
    /// with [`Self::sync_replayed`].
    pub async fn replay_write(
        &self,
        id: u32,
        offset: u64,
        bytes: Bytes,
    ) -> Result<(), PackfileError> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        if id == 0 {
            return Err(PackfileError::internal("Invalid packfile"));
        }
        if !path.exists() {
            IoDriver::global()
                .create_dir_all(path)
                .await
                .internal("Failed to create packstore directory")?;
        }
        let file_path = path.join(id.to_string());
        let file = IoDriver::global()
            .open(
                file_path.as_path(),
                &OpenOptions::new().read(true).write(true).create(true),
            )
            .await
            .internal_with(|| format!("Failed opening packstore file {}", file_path.display()))?;
        packfile_write(&file, bytes, offset as usize).await
    }

    /// Sync packfiles `ids` after [`Self::replay_write`], and the directory that
    /// names them.
    pub async fn sync_replayed(&self, ids: &[u32]) -> Result<(), PackfileError> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        for id in ids {
            let file_path = path.join(id.to_string());
            let file = IoDriver::global()
                .open(
                    file_path.as_path(),
                    &OpenOptions::new().read(true).write(true),
                )
                .await
                .internal_with(|| {
                    format!("Failed opening packstore file {}", file_path.display())
                })?;
            file.sync_data()
                .await
                .internal("Failed syncing replayed packstore file")?;
        }
        #[cfg(target_family = "unix")]
        {
            let directory = path.clone();
            tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all())
                .await
                .internal("Failed syncing packstore directory")?
                .internal("Failed syncing packstore directory")?;
        }
        Ok(())
    }

    pub async fn total_size(&self) -> Result<usize, PackfileError> {'''),
]
