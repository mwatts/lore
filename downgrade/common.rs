// Shared by the downgrade writer (new Lore) and reader (Lore 24545866). Scratch, not committed.
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use lore_base::types::KeyType;
use lore_storage::Address;
use lore_storage::Context;
use lore_storage::Fragment;
use lore_storage::Hash;
use lore_storage::ImmutableStore;
use lore_storage::ImmutableStoreSettings;
use lore_storage::LocalImmutableStore;
use lore_storage::LocalMutableStore;
use lore_storage::MutableStore;
use lore_storage::MutableStoreSettings;
use lore_storage::Partition;

const COUNT: u64 = 16;

fn partition() -> Partition {
    Partition::from([7u8; 16])
}
fn payload(n: u64) -> Bytes {
    Bytes::from((0..4096u64).map(|i| (i.wrapping_mul(2_654_435_761) ^ n) as u8).collect::<Vec<u8>>())
}
fn address(n: u64) -> Address {
    Address { hash: lore_storage::hash_slice(&payload(n)), context: Context::default() }
}
fn key() -> Hash {
    lore_storage::hash_slice(b"branch tip")
}
fn value() -> Hash {
    lore_storage::hash_slice(b"revision 16")
}
