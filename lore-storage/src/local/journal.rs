// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
//! A store's write-ahead journal: one append-only file whose records a flush
//! appends in one write and makes durable with one sync.
//!
//! Without it, a flush makes each change durable where it finally lives: every
//! packfile that took a payload is synced, and every index bucket that changed
//! is rewritten whole through a temporary file, a sync, a rename, and a
//! directory sync. A small revision touches a dozen groups, so a flush costs a
//! dozen of each. With it, a flush appends what changed to this file and syncs
//! it once; the packfiles and buckets catch up in a fold, when the journal has
//! grown past a threshold, after which the journal is emptied. A store that
//! opens with its journal replays it into memory and appends after it, so an
//! open writes nothing; one that opens without its journal folds it.
//!
//! A record is a fixed header (magic, kind, body length, and a checksum of the
//! kind, length, and body) followed by its body. A crash can leave a torn last
//! record: reading stops at the first record that is short or fails its
//! checksum, and the file is cut back to the records before it. Replaying a
//! record must be idempotent, since a crash during a fold replays records the
//! fold had already applied.

use std::path::Path;
use std::path::PathBuf;

use bytes::Bytes;
use lore_io::IoDriver;
use lore_io::IoFile;
use lore_io::OpenOptions;

/// "LJR1", little endian.
const MAGIC: u32 = 0x3152_4a4c;
const HEADER_SIZE: usize = 24;

/// What a record holds. The store that owns a journal decides what each kind's
/// body means; the journal only frames and checks them.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// A payload appended to a packfile: group, packfile id, offset, bytes.
    Payload = 1,
    /// A payload range a packfile zeroed: group, packfile id, offset, length.
    Zero = 2,
    /// An index entry upserted in the immutable store.
    ImmutableEntry = 3,
    /// An index entry upserted in the mutable store.
    MutableEntry = 4,
}

impl RecordKind {
    fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::Payload),
            2 => Some(Self::Zero),
            3 => Some(Self::ImmutableEntry),
            4 => Some(Self::MutableEntry),
            _ => None,
        }
    }
}

/// One record read back from a journal.
#[derive(Debug, Clone)]
pub struct Record {
    pub kind: RecordKind,
    pub body: Bytes,
}

fn checksum(kind: u32, body: &[&[u8]]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&kind.to_le_bytes());
    let len: usize = body.iter().map(|part| part.len()).sum();
    hasher.update(&(len as u32).to_le_bytes());
    for part in body {
        hasher.update(part);
    }
    let hash = hasher.finalize();
    u64::from_le_bytes(hash.as_bytes()[..8].try_into().expect("8 bytes"))
}

/// Append one framed record, whose body is the concatenation of `body`, to `out`.
pub fn encode(out: &mut Vec<u8>, kind: RecordKind, body: &[&[u8]]) {
    let len: usize = body.iter().map(|part| part.len()).sum();
    out.reserve(HEADER_SIZE + len);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&(kind as u32).to_le_bytes());
    out.extend_from_slice(&(len as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&checksum(kind as u32, body).to_le_bytes());
    for part in body {
        out.extend_from_slice(part);
    }
}

/// Every whole record at the start of `bytes`, and the length they take.
pub fn decode(bytes: &Bytes) -> (Vec<Record>, usize) {
    let mut records = Vec::new();
    let mut at = 0usize;
    while bytes.len() - at >= HEADER_SIZE {
        let word = |offset: usize| {
            u32::from_le_bytes(
                bytes[at + offset..at + offset + 4]
                    .try_into()
                    .expect("4 bytes"),
            )
        };
        if word(0) != MAGIC {
            break;
        }
        let kind = word(4);
        let len = word(8) as usize;
        let check = u64::from_le_bytes(bytes[at + 16..at + 24].try_into().expect("8 bytes"));
        let Some(end) = (at + HEADER_SIZE).checked_add(len) else {
            break;
        };
        if end > bytes.len() {
            break;
        }
        let body = bytes.slice(at + HEADER_SIZE..end);
        if checksum(kind, &[&body]) != check {
            break;
        }
        let Some(kind) = RecordKind::from_u32(kind) else {
            break;
        };
        records.push(Record { kind, body });
        at = end;
    }
    (records, at)
}

/// Whether a journal file exists at `path` and holds anything.
pub fn holds_records(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
}

/// The journal file of one store.
pub struct Journal {
    path: PathBuf,
    file: IoFile,
    size: u64,
}

impl Journal {
    /// Open the journal at `path`, creating it (and making its name durable) when
    /// absent, and return the records it holds. A torn tail is cut off.
    pub async fn open(path: PathBuf) -> std::io::Result<(Self, Vec<Record>)> {
        let existed = IoDriver::global().metadata(&path).await.is_ok();
        let file = IoDriver::global()
            .open(
                &path,
                &OpenOptions::new().read(true).write(true).create(true),
            )
            .await?;
        if !existed {
            file.sync_all().await?;
            sync_parent(&path).await?;
        }
        let len = file.metadata().await?.len() as usize;
        let bytes = if len == 0 {
            Bytes::new()
        } else {
            file.read_exact_at(len, 0).await?
        };
        let (records, valid) = decode(&bytes);
        if valid != len {
            lore_base::lore_warn!(
                "Journal {} ends in {} bytes of a torn record, cutting them off",
                path.display(),
                len - valid
            );
            file.set_len(valid as u64).await?;
            file.sync_data().await?;
        }
        Ok((
            Self {
                path,
                file,
                size: valid as u64,
            },
            records,
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Bytes the journal holds.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Append `records` (already framed with [`encode`]) in one write, and make
    /// them durable with one sync when `sync_data` is set.
    pub async fn append(&mut self, records: Vec<u8>, sync_data: bool) -> std::io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let len = records.len() as u64;
        if let Err(error) = self
            .file
            .write_all_at(Bytes::from(records), self.size)
            .await
        {
            // Cut a partial append off, so a later append does not land after it.
            let _ = self.file.set_len(self.size).await;
            return Err(error);
        }
        if sync_data && let Err(error) = self.file.sync_data().await {
            let _ = self.file.set_len(self.size).await;
            return Err(error);
        }
        self.size += len;
        Ok(())
    }

    /// Empty the journal once everything it holds is durable elsewhere.
    pub async fn clear(&mut self) -> std::io::Result<()> {
        if self.size == 0 {
            return Ok(());
        }
        self.file.set_len(0).await?;
        self.file.sync_data().await?;
        self.size = 0;
        Ok(())
    }
}

async fn sync_parent(path: &Path) -> std::io::Result<()> {
    #[cfg(target_family = "unix")]
    if let Some(parent) = path.parent().map(Path::to_path_buf) {
        tokio::task::spawn_blocking(move || std::fs::File::open(parent)?.sync_all())
            .await
            .map_err(std::io::Error::other)??;
    }
    #[cfg(not(target_family = "unix"))]
    let _ = path;
    Ok(())
}

/// The body of a [`RecordKind::Payload`] record.
pub fn payload_body(group: u32, pack_id: u32, offset: u64) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&group.to_le_bytes());
    out[4..8].copy_from_slice(&pack_id.to_le_bytes());
    out[8..].copy_from_slice(&offset.to_le_bytes());
    out
}

/// Parse a [`RecordKind::Payload`] body: group, packfile id, offset, bytes.
pub fn parse_payload(body: &Bytes) -> Option<(usize, u32, u64, Bytes)> {
    if body.len() < 16 {
        return None;
    }
    let group = u32::from_le_bytes(body[..4].try_into().ok()?) as usize;
    let pack_id = u32::from_le_bytes(body[4..8].try_into().ok()?);
    let offset = u64::from_le_bytes(body[8..16].try_into().ok()?);
    Some((group, pack_id, offset, body.slice(16..)))
}

/// The body of a [`RecordKind::Zero`] record.
pub fn zero_body(group: u32, pack_id: u32, offset: u64, len: u64) -> [u8; 24] {
    let mut out = [0u8; 24];
    out[..4].copy_from_slice(&group.to_le_bytes());
    out[4..8].copy_from_slice(&pack_id.to_le_bytes());
    out[8..16].copy_from_slice(&offset.to_le_bytes());
    out[16..].copy_from_slice(&len.to_le_bytes());
    out
}

/// Parse a [`RecordKind::Zero`] body: group, packfile id, offset, length.
pub fn parse_zero(body: &Bytes) -> Option<(usize, u32, u64, u64)> {
    if body.len() != 24 {
        return None;
    }
    Some((
        u32::from_le_bytes(body[..4].try_into().ok()?) as usize,
        u32::from_le_bytes(body[4..8].try_into().ok()?),
        u64::from_le_bytes(body[8..16].try_into().ok()?),
        u64::from_le_bytes(body[16..24].try_into().ok()?),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(records: &[(RecordKind, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (kind, body) in records {
            encode(&mut out, *kind, &[body]);
        }
        out
    }

    #[test]
    fn decode_reads_back_every_whole_record() {
        let bytes = Bytes::from(framed(&[
            (RecordKind::Payload, b"first"),
            (RecordKind::Zero, b""),
            (RecordKind::MutableEntry, &[7u8; 100]),
        ]));
        let (records, valid) = decode(&bytes);
        assert_eq!(valid, bytes.len());
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].kind, RecordKind::Payload);
        assert_eq!(records[0].body.as_ref(), b"first");
        assert!(records[1].body.is_empty());
        assert_eq!(records[2].body.as_ref(), &[7u8; 100]);
    }

    #[test]
    fn decode_stops_at_a_torn_or_damaged_record() {
        let whole = framed(&[
            (RecordKind::Payload, b"kept"),
            (RecordKind::Payload, b"torn tail"),
        ]);
        let first_len = framed(&[(RecordKind::Payload, b"kept")]).len();
        for cut in first_len..whole.len() {
            let (records, valid) = decode(&Bytes::copy_from_slice(&whole[..cut]));
            assert_eq!(records.len(), 1, "cut at {cut}");
            assert_eq!(valid, first_len, "cut at {cut}");
        }
        let mut damaged = whole.clone();
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        let (records, valid) = decode(&Bytes::from(damaged));
        assert_eq!(records.len(), 1);
        assert_eq!(valid, first_len);
    }

    #[tokio::test]
    async fn open_cuts_a_torn_tail_and_appends_after_the_whole_records() {
        let dir = crate::test_util::TempDir::new("lore-journal-");
        let path = dir.path().join("journal");
        let mut bytes = framed(&[(RecordKind::Payload, b"whole")]);
        let whole = bytes.len();
        bytes.extend_from_slice(&framed(&[(RecordKind::Payload, b"torn")])[..10]);
        std::fs::write(&path, &bytes).unwrap();

        let (mut journal, records) = Journal::open(path.clone()).await.unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(journal.size(), whole as u64);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), whole as u64);

        journal
            .append(framed(&[(RecordKind::Zero, b"next")]), true)
            .await
            .unwrap();
        drop(journal);
        let (mut journal, records) = Journal::open(path.clone()).await.unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].body.as_ref(), b"next");

        journal.clear().await.unwrap();
        drop(journal);
        let (_journal, records) = Journal::open(path).await.unwrap();
        assert!(records.is_empty());
    }
}
