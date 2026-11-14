PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/immutable_store.rs'
EDITS = [
('''            entry.data.merge_from_copy_source(source_data, durable);
            if entry.data != before {
                entry.data.last_access = Self::last_access();
                group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
            }
            return Ok(());
        }''','''            entry.data.merge_from_copy_source(source_data, durable);
            if entry.data != before {
                entry.data.last_access = Self::last_access();
                group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
                self.journal_entry_changed(
                    destination_address.hash.data()[0] as usize,
                    destination_address.hash,
                );
            }
            return Ok(());
        }'''),
('''        bucket.entry.push(ImmutableStoreEntry {
            address: destination_address,
            partition: destination_partition,
            data,
        });
        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);

        Ok(())''','''        bucket.entry.push(ImmutableStoreEntry {
            address: destination_address,
            partition: destination_partition,
            data,
        });
        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
        self.journal_entry_changed(
                    destination_address.hash.data()[0] as usize,
                    destination_address.hash,
                );

        Ok(())'''),
]
