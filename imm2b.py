PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/immutable_store.rs'
EDITS = [
('''        if is_last_fragment && entry.data.pack_file != 0 {
            lore_base::lore_debug!(
                "Fragment payload has no other references, obliterating from packstore"
            );

            stats.num_payloads.fetch_add(1, atomic::Ordering::Relaxed);

            group
                .packstore
                .obliterate(
                    entry.data.pack_file,
                    entry.data.pack_offset,
                    entry.data.size_payload,
                )
                .await
                .forward::<StoreError>("Failed to obliterate payload from pack store.")?;
        }
''','''        let mut zeroed = None;
        if is_last_fragment && entry.data.pack_file != 0 {
            lore_base::lore_debug!(
                "Fragment payload has no other references, obliterating from packstore"
            );

            stats.num_payloads.fetch_add(1, atomic::Ordering::Relaxed);

            group
                .packstore
                .obliterate(
                    entry.data.pack_file,
                    entry.data.pack_offset,
                    entry.data.size_payload,
                )
                .await
                .forward::<StoreError>("Failed to obliterate payload from pack store.")?;
            zeroed = Some((
                entry.data.pack_file,
                entry.data.pack_offset,
                entry.data.size_payload,
            ));
        }
'''),
('''        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
        drop(bucket);

        let mut flush = group.flush.lock().await;
        let _ = flush.try_join_next();

        if flush.is_empty() {
            let weak_self = Arc::downgrade(&self);
            lore_base::lore_spawn!(
                flush,
                ImmutableStoreGroup::flush_delayed(
                    weak_self,
                    group_index,
                    self.settings.flush_delay_seconds,
                )
            );
        }

        Ok(())
    }

    pub async fn load(''','''        group.dirty[bucket_index].store(true, atomic::Ordering::Relaxed);
        drop(bucket);

        // The journal may hold the payload just zeroed; record the zeroing after it, so a
        // replay does not bring the payload back. Taken after the bucket lock is released:
        // a flush holds the journal while it takes bucket locks.
        if let Some((pack_file, pack_offset, size_payload)) = zeroed {
            self.journal_zero(group_index, pack_file, pack_offset, size_payload)
                .await
                .forward::<StoreError>("Failed to journal an obliterated payload.")?;
        }

        let mut flush = group.flush.lock().await;
        let _ = flush.try_join_next();

        if flush.is_empty() {
            let weak_self = Arc::downgrade(&self);
            lore_base::lore_spawn!(
                flush,
                ImmutableStoreGroup::flush_delayed(
                    weak_self,
                    group_index,
                    self.settings.flush_delay_seconds,
                )
            );
        }

        Ok(())
    }

    pub async fn load('''),
]
