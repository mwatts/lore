p='/home/mwatts/code/github/mwatts/ashlar-r4-lore/docs/design-seeding-footprint.md'
s=open(p).read()
i=s.index('## Lore track results')
j=s.index('## Not measured')
new='''## Lore track results

Changes 1 to 5 landed on 2026-11-06, on the same host, and a fix round followed on 2026-11-07. Lore fork `mwatts/lore` branch `ashlar` (not pushed): `1772d376` change 3, `d64d34ef` change 2, `fb9e3f45` change 1, `cbb2efc5`, which looks a large batch's names up in a table per directory instead of a sibling walk per file (an Ashlar upgrade of 2,100 Notes needed it to open within its 5 s budget), and from the fix round `f62f840` (a store with no bucket yet opens as a new store), `7b476ed` (a journaled store is refused by an older Lore), and `c87a957` (a test that revisions across journal folds reopen whole). Cairn branch `r4-lore` (not pushed): `364a2c5` changes 3 to 5, `0067621` and `f587778` the Lore pin. Every save keeps its history, `sync_data` is still on for every Lore call, and the `/meta` path layout is unchanged.

### What each change does

- **Change 3.** Lore's `revision::commit_contents` commits bytes held in memory as one revision: it loads the current tree, applies the writes and removals, and freezes it with `commit_in_memory_revision_with` and `InMemoryCommitRecovery::Discard`, which skips the recovery snapshot. Cairn's `commit_changes_in` calls it in place of stage and commit, for one file or many.
- **Change 4.** That path is one call, so nothing is staged between calls and Cairn writes no pending-batch marker. `commit_file_from`, which streams a large import from disk, keeps the staging path and its marker.
- **Change 5.** That path writes no `meta/` checkout and stages nothing from the working tree, so Lore writes no clock probe. A checkout left at a path the batch changes is deleted first. Content residency checks the tip leaf's hash when there is no checkout to hash.
- **Change 2.** A journaled Lore store keeps the payloads it stores in memory and appends the ones stored since the last flush to one journal file per store, in one write, synced once. They reach their packfiles in a fold: one write and one sync per packfile.
- **Change 1.** The same journal takes the index entries a flush changed, in the same write and sync, for the immutable and the mutable store. Buckets are written only in a fold. A fold happens when the journal passes 16 MiB (4 MiB for the mutable store), when a flush changes more than 256 KiB of entries (a revision of thousands of objects), after an eviction, compaction, or obliteration, before a compaction drops a packfile, and when the store opens.

### Crash safety, and an older Lore

A crash before a commit's call publishes leaves the tip where it was; payloads it stored are unreferenced. Every fold empties the journal and no bucket is written between folds, so replaying the journal on open is idempotent: payload records write the same bytes at the same offsets, entry records set entries to the values they had when journaled. A torn last record is cut off. A store opened without the journal setting still folds a journal it finds.

A Lore binary without these commits reads only packfiles and buckets. Before `7b476ed` it opened a journaled store and went without what the journal held: a store written with unfolded journals and opened by Lore `24545866` read back 0 of 16 fragments and no branch tip (measured, a scripted run with both binaries). That Lore refuses an immutable store whose `info` file names any version but 0. A journaled open now writes version 1 there, durably, before its journal takes a record, so `24545866` fails to open the same store with "Info file has unsupported version 1, expected 0" (measured). Every repository opens its immutable store, so this also covers the mutable store's journal. The stamp stays after a fold: an archive this Lore has opened cannot go back to an older one.

Tests (all pass): Lore `lore-storage/tests/journal_crash.rs` kills a writer with SIGKILL in ten rounds, mid-flush and mid-fold, reopens, and checks every flushed round reads back whole and every key holds its last flushed value; it fails if replay skips payloads or entries (checked by mutation). `lore-storage/tests/journal_reopen.rs` checks that a store closed before its first fold reopens with a new store's bucket layout, and that a journaled store's `info` file names a version an older Lore refuses; each fails without its fix (checked by mutation). `lore/tests/revision_commit_contents.rs` has a writer process commit 40 revisions of 1 MiB across a journal fold, keeping its stores open as a Cairn session does, and exit without closing anything; a new process reads every file at every revision; it fails if the open skips the journal's payloads (checked by mutation). Cairn `cairn-lore/tests/crash_reopen.rs` kills a writer in eight rounds across the three write paths (one file, several files with a removal, and the streaming import) and checks the root reopens at the last logged revision or its child, every logged revision is whole, and the next commit lands.

### The reopen after seeding

The change 2 run at 50,000 Records seeded, then failed to reopen: "Database at .../.cairn/projection/gen-2/db/LOCK is already locked by another process". That is the projection's SurrealKV lock, not a Lore store. It did not happen again: the probe now reopens at once and reports any failure, and four runs at 50,000 with every change, one of them beside eight busy CPU loops and four synced writers (seed 165 s), reopened at the first attempt, with the close and drop taking 17 to 398 ms (measured).

The cause is inferred from the code, not measured. Dropping the projection's store waits for its datastore to close, but `cairn-surreal` stops waiting after 10 s (`CLOSE_BOUND`, `crates/cairn-surreal/src/store.rs:159` and `:286`) and lets an open proceed. SurrealKV's close waits for its background tasks, a running compaction among them, and flushes its memtables (`surrealkv-0.21.4/src/lsm.rs:1355`). The failed run seeded in 126 s on a host at a load average near 40, so a close that outlasted 10 s while compacting fits. A second path that fits: a drop on the engine's own runtime does not wait at all (`store.rs:591`). The fix is in `cairn-surreal`, which the projection track is changing, so it is not in these commits: an open should wait until the datastore has released its lock, however long the close takes, or report that the projection is still closing. Lore's reopen is covered by the tests above.

### Bytes per save and per seeded Record

Measured with the probe (`R4_SINGLES=20 R4_EDITS=20`, release build). The probe closes and reopens the archive around each save phase, outside what the phase measures, so that "kept" is the growth of Lore's packfiles after the journal is folded. Written is `write_bytes` per save, kept is packfile growth per save, seed is `write_bytes` of the seed per Record.

| Lore and Cairn | 5,000: create written / kept | 5,000: edit written / kept | 5,000: seed | 50,000: create written / kept | 50,000: edit written / kept | 50,000: seed |
| --- | --- | --- | --- | --- | --- | --- |
| Before (this run's baseline at 5,000; Step 0 at 50,000) | 3,293 / 115 KB | 2,882 / 60 KB | 92.6 KB | 3,541 / 117 KB | 3,144 / 69 KB | 126.7 KB |
| Changes 3 to 5 | 2,002 / 59.7 KB | 1,601 / 31.9 KB | 59.2 KB | 2,055 / 62.3 KB | 1,588 / 37.5 KB | 92.5 KB |
| And change 2 | 1,611 / 59.3 KB | 1,384 / 31.6 KB | 49.3 KB | not measured (above) | not measured | 82.0 KB |
| And change 1 (`cbb2efc5`) | 234 / 59.4 KB | 224 / 31.8 KB | 32.7 KB | 264 / 62.2 KB | 176 / 37.4 KB | 120.6 KB (122.3 KB on 2026-11-07) |
| And the fix round (`c87a957`) | 233 / 59.5 KB | 165 / 31.8 KB | 32.4 KB | 198 / 62.0 KB | 174 / 37.4 KB | 72.1 KB |

All measured. The fix round's rows ran on a quiet host (load average under 2). The written figures leave out folds; the next section has them.

### Why the seed got worse with change 1, and the fix

A Lore repository is created and closed before it takes its first revision. A journaled store writes no bucket before its first fold, so it closed with group directories (the packstore makes them on first open) and no bucket file or level marker in any of them. The next open read that as a store written before bucket levels existed (`lore-storage/src/local/immutable_store.rs`, the serialize-version decision): it kept the pre-fan-out format, started every group at 256 buckets, and wrote no level marker, so the store stayed that way. Each journaled repository had 65,536 index buckets where one written without the journal had 256, fanning out as it grows. A fold rewrites every dirty bucket, each through a temporary file, a sync, a rename, and a directory sync, and at that layout a fold touched about 15,000 bucket files (measured with a fold log: 15,243, 15,201, and 14,355 in the three folds of a 50,000 seed). Lore's I/O threads wrote 2,813 MB during that run, against 1,325 MB with changes 3 to 5 and no journal (measured). At 5,000 the seed journals about 5 MB, so no fold fell inside it and the seed looked better; its fold ran at the next open.

`f62f840` treats an index directory with no bucket and no marker in any group as a new store, so groups start at one bucket and fan out as they grow. The same three folds now touch 256 buckets each, and Lore's I/O threads write 300 MB for the 50,000 run, seed and saves together (measured). The seed per Record at 50,000 fell from 122.3 to 72.1 KB, below the 82.0 KB of change 2 alone (measured). Of the 72.1 KB, the projection writes about 68 KB (3,406 MB on its threads); Lore's share is about 6 KB. Fold frequency, journal growth, and replay cost were not the cause: the journal folded at 16.9 to 17.4 MB each time, as designed. An archive a Lore at `fb9e3f45` to `cbb2efc5` created keeps the 256-bucket layout; none was kept.

### What change 1 defers

Change 1's per-save figures leave out the fold. A save's journal record is about 55 KB, so a 16 MiB journal folds about every 300 saves, and the fold rewrites every bucket those saves touched and writes each packfile's tail, with a sync each. With the fix round, 600 creates, two folds among them, wrote 410 KB per create at 5,000 Records and 399 KB at 50,000, against 233 KB and 198 KB between folds (measured; a stalled probe ran beside these two runs, writing about 0.2 MB/s). A fold now writes about 50 to 60 MB (estimated from those figures), against about 207 MB at 5,000 and 276 MB at 50,000 before it (measured). Per create with its share of the folds: about 0.4 MB, against 0.9 to 1.16 MB at `cbb2efc5`, 1.6 MB with change 2 alone, and 3.3 MB before.

The fold's cost is per file touched, not per entry: 300 one-object saves touch every one of the 256 groups, so each fold rewrites every group's bucket and packfile tail, with two syncs per bucket. A fold that synced its buckets once for all of them, or bucket files that take appends, would remove most of what is left; neither is in these commits.

### Kept per save, and the budget

Kept per create fell from 115 to 59 KB and per edit from 60 to 32 KB at 5,000 (62 and 37 KB at 50,000), all from change 3: the staged state is no longer written. Changes 1 and 2 move where payloads are written, not how many. The 32 KiB budget for growth per save still needs change 6 (smaller node blocks), as the owner's decision expected: the four 512-node blocks of the revision tree are now most of a save's kept bytes.

### Against the pass test's budgets

At 50,000 Records: bytes written per one-object save is 198 KB between folds and about 0.4 MB with its share of the folds (budget 512 KiB), both within it (measured). Kept per save is 62 KB per create (budget 32 KiB), which needs change 6. The seed writes 72.1 KB per Record (budget 64 KiB); about 68 KB of it is the projection's, which recommendations 2, 3, and 5 address in Cairn beside these.

'''
s=s[:i]+new+s[j:]
open(p,'w').write(s)
