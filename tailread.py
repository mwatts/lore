PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/packstore.rs'
EDITS = [
('''    /// The bytes `[offset, offset + size)` if they lie in one payload of the tail.
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
''','''    /// The bytes `[offset, offset + size)` when any of them are in the tail, which
    /// starts where the file's written bytes end: the part before the tail is read
    /// from the file. `None` when the range ends before the tail.
    async fn read_with_tail(
        &self,
        offset: usize,
        size: usize,
    ) -> Option<Result<Bytes, PackfileError>> {
        let start = self.tail.first()?.offset;
        let end = offset.checked_add(size)?;
        if end <= start {
            return None;
        }
        if offset >= start
            && let Ok(index) = self.tail.binary_search_by_key(&offset, |item| item.offset)
            && size <= self.tail[index].bytes.len()
        {
            return Some(Ok(self.tail[index].bytes.slice(..size)));
        }
        let mut out = BytesMut::with_capacity(size);
        if offset < start {
            let Some(file) = self.file.as_ref() else {
                return Some(Err(PackfileError::internal(
                    "Failed reading from packstore, boundary violation",
                )));
            };
            match packfile_read(file, offset, start - offset).await {
                Ok(bytes) => out.extend_from_slice(&bytes),
                Err(err) => return Some(Err(err)),
            }
        }
        for item in &self.tail {
            let item_end = item.offset + item.bytes.len();
            if item_end <= offset.max(start) || item.offset >= end {
                continue;
            }
            let from = offset.max(item.offset) - item.offset;
            let to = end.min(item_end) - item.offset;
            out.extend_from_slice(&item.bytes[from..to]);
        }
        if out.len() != size {
            return Some(Err(PackfileError::internal(
                "Failed reading from packstore tail, boundary violation",
            )));
        }
        Some(Ok(out.freeze()))
    }
'''),
('''        if let Some(bytes) = packfile.tail_slice(offset, size) {
            return Ok(bytes);
        }
''','''        if let Some(bytes) = packfile.read_with_tail(offset, size).await {
            return bytes;
        }
'''),
('''        if let Some(bytes) = packfile.tail_slice(offset, size) {
            let Some(target) = dst.as_mut_slice().get_mut(..size) else {''','''        if let Some(bytes) = packfile.read_with_tail(offset, size).await {
            let bytes = bytes?;
            let Some(target) = dst.as_mut_slice().get_mut(..size) else {'''),
]
