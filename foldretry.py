PATH = '/home/mwatts/code/github/mwatts/lore/lore-storage/src/local/immutable_store.rs'
EDITS = [
('''        for group in &self.group {
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
            LocalImmutableStoreError::internal_with_context(
                err,
                "Failed to empty the store journal",
            )
        })
    }''','''        let folded = async {
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
                LocalImmutableStoreError::internal_with_context(
                    err,
                    "Failed to empty the store journal",
                )
            })
        }
        .await;
        if folded.is_err() {
            // The pending lists are gone, so only a fold makes those changes durable:
            // the next flush tries again.
            self.journal_needs_fold
                .store(true, atomic::Ordering::Relaxed);
        }
        folded
    }'''),
]
