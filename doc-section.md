## Lore track results

Changes 1 to 5 landed on 2026-11-06, on the same host. Lore fork `mwatts/lore` branch `ashlar` (not pushed): `1772d376` change 3, `d64d34ef` change 2, `fb9e3f45` change 1. Cairn branch `r4-lore` (not pushed): `364a2c5` changes 3 to 5, `8bc50ca` the Lore pin. Every save keeps its history, `sync_data` is still on for every Lore call, and the `/meta` path layout is unchanged.

### What each change does

- **Change 3.** Lore's `revision::commit_contents` commits bytes held in memory as one revision: it loads the current tree, applies the writes and removals, and freezes it with `commit_in_memory_revision_with(…, InMemoryCommitRecovery::Discard)`, which skips the recovery snapshot. Cairn's `commit_changes_in` calls it in place of stage and commit, for one file or many.
- **Change 4.** That path is one call, so nothing is staged between calls and Cairn writes no pending-batch marker. `commit_file_from`, which streams a large import from disk, keeps the staging path and its marker.
- **Change 5.** That path writes no `meta/` checkout and stages nothing from the working tree, so Lore writes no clock probe. A checkout left at a path the batch changes is deleted first. Content residency checks the tip leaf's hash when there is no checkout to hash.
- **Change 2.** A journaled Lore store keeps the payloads it stores in memory and appends the ones stored since the last flush to one journal file per store, in one write, synced once. They reach their packfiles in a fold: one write and one sync per packfile.
- **Change 1.** The same journal takes the index entries a flush changed, in the same write and sync, for the immutable and the mutable store. Buckets are written only in a fold. A fold happens when the journal passes 16 MiB (4 MiB for the mutable store), when a flush changes more than 256 KiB of entries (a revision of thousands of objects), after an eviction, compaction, or obliteration, before a compaction drops a packfile, and when the store opens.

### Crash safety

A crash before a commit's call publishes leaves the tip where it was; payloads it stored are unreferenced. Every fold empties the journal and no bucket is written between folds, so replaying the journal on open is idempotent: payload records write the same bytes at the same offsets, entry records set entries to the values they had when journaled. A torn last record is cut off. A store opened without the journal setting still folds a journal it finds. A Lore binary without these commits would ignore the journal and lose what it holds, so an archive these commits wrote must not be opened by an older Lore.

Tests (all pass): Lore `lore-storage/tests/journal_crash.rs` kills a writer with SIGKILL in ten rounds, mid-flush and mid-fold, reopens, and checks every flushed round reads back whole and every key holds its last flushed value; it fails if replay skips payloads or entries (checked by mutation). Cairn `cairn-lore/tests/crash_reopen.rs` kills a writer in eight rounds across the three write paths (one file, several files with a removal, and the streaming import) and checks the root reopens at the last logged revision or its child, every logged revision is whole, and the next commit lands.

### Bytes per save and per seeded Record

Measured with the probe (`R4_SINGLES=20 R4_EDITS=20`, release build). The probe now closes and reopens the archive around each save phase, outside what the phase measures, so that "kept" is the growth of Lore's packfiles after the journal is folded. Written is `write_bytes` per save, kept is packfile growth per save, seed is `write_bytes` of the seed per Record.

| Lore and Cairn | 5,000: create written / kept | 5,000: edit written / kept | 5,000: seed | 50,000: create written / kept | 50,000: edit written / kept | 50,000: seed |
| --- | --- | --- | --- | --- | --- | --- |
| Before (this run's baseline at 5,000; Step 0 at 50,000) | 3,293 / 115 KB | 2,882 / 60 KB | 92.6 KB | 3,541 / 117 KB | 3,144 / 69 KB | 126.7 KB |
| Changes 3 to 5 | 2,002 / 59.7 KB | 1,601 / 31.9 KB | 59.2 KB | 2,055 / 62.3 KB | 1,588 / 37.5 KB | 92.5 KB |
| And change 2 | 1,611 / 59.3 KB | 1,384 / 31.6 KB | 49.3 KB | not measured (below) | not measured | 82.0 KB |
| And change 1 | 234 / 59.4 KB | 224 / 31.8 KB | 32.7 KB | 264 / 62.2 KB | 176 / 37.4 KB | 120.6 KB |

All measured. The change 2 run at 50,000 seeded, then failed to reopen the archive (the projection's lock was still held), so its saves were not measured; the probe now retries the reopen.

### What change 1 defers

Change 1's per-save figures leave out the fold. A save's journal record is about 55 KB, so a 16 MiB journal folds about every 300 saves, and the fold rewrites every bucket those saves touched, with a sync each. Measured at 5,000 Records: 300 creates wrote 65.0 MB, and the fold that emptied their journal (the reopen after them, less the 3 MB a reopen without a fold writes) wrote about 207 MB. That is about 0.9 MB per create with its share of the fold, against 1.6 MB with change 2 alone and 3.3 MB before. At 50,000 Records, 300 creates wrote 72.0 MB and their fold about 276 MB: about 1.16 MB per create, against 2.1 MB with changes 3 to 5 and 3.5 MB before. These 300-create runs shared the host with another agent's builds, so they are the least exact figures here.

The fold's cost is per bucket touched, not per entry: a few hundred one-object saves touch thousands of bucket files, and each is rewritten through a temporary file and two syncs. A fold that synced its buckets once for all of them, or bucket files that take appends, would remove most of what is left; neither is in these commits.

The seed at 50,000 Records is worse with change 1 than with change 2 alone: 120.6 KB per Record against 82.0 KB (measured). At 5,000 it is better (32.7 against 49.3 KB). The cause was not established. A fold rewrites every bucket the journal touched since the last one, and a seed's revisions touch most of them, so folding every 16 MiB saves little at that size; that does not explain a loss. Two variants that wrote a bulk revision's buckets at its flush were measured and dropped (129.5 KB per Record at 50,000). Until the cause is found, `seed_records` (an Ashlar test helper) pays more at 50,000 with change 1 than without it; the owner's saves pay less.

### Kept per save, and the budget

Kept per create fell from 115 to 59 KB and per edit from 60 to 32 KB at 5,000 (62 and 38 KB at 50,000), all from change 3: the staged state is no longer written. Changes 1 and 2 move where payloads are written, not how many. The 32 KiB budget for growth per save still needs change 6 (smaller node blocks), as the owner's decision expected: the four 512-node blocks of the revision tree are now most of a save's kept bytes.

### Against the pass test's budgets

At 50,000 Records: bytes written per one-object save is 264 KB between folds (budget 512 KiB), and about 1.16 MB with its share of the fold, over the budget. Kept per save is 62 KB per create (budget 32 KiB), which needs change 6. The seed writes 120.6 KB per Record (budget 64 KiB); the projection's share of it is recommendations 2, 3, and 5, in Cairn beside these, and Lore's share needs the seed regression above explained.
