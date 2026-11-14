PATH = '/home/mwatts/code/github/mwatts/ashlar/vendor/cairn/.claude/worktrees/agent-a73219e9c3799f25c/docs/lore-embed.md'
EDITS = [
("| **Rev** | `24545866cf1a8f5ae3f0a996270e7aa7cdd868ac` |",
 "| **Rev** | `ad2accbdf07880d667481df57222ae95aec06fae` (branch `ashlar`; not pushed when this was written, so the rev resolves only once `mwatts/lore` has it) |"),
("| **Base** | EpicGames/lore `5fa7453f95b24700c5e8ff63678d530b55df1ea0` plus the name-lookup commits and a dependency fix |",
 "| **Base** | EpicGames/lore `5fa7453f95b24700c5e8ff63678d530b55df1ea0` plus the name-lookup commits and a dependency fix (`24545866`), plus the three R4 commits below |"),
("| **Upstream** |",
 "| **R4 commits** | Ashlar gate R4, Step 0 changes 1 to 3. `1772d376` lore-revision: commit in-memory contents without a staged state (`revision::commit_contents`, and `InMemoryCommitRecovery::Discard` on `commit_in_memory_revision_with`). `d64d34ef` lore-storage: a flush's payloads are durable with one journal sync. `ad2accbd` lore-storage: index changes are durable by journal, not bucket rewrites. The journal is `ImmutableStoreSettings::journal` and `MutableStoreSettings::journal`, on for client repository stores. Each is offered upstream to EpicGames/lore; the fork carries them until upstream takes them. A Lore binary without these commits ignores a store's `journal` file and loses what it holds, so a repository a journaled binary wrote must not be opened by an older one. |\n| **Upstream** |"),
('rev = "24545866cf1a8f5ae3f0a996270e7aa7cdd868ac" }',
 'rev = "ad2accbdf07880d667481df57222ae95aec06fae" }'),
("Offline `repository::create`; `file::stage` + `revision::commit`;",
 "Offline `repository::create`; `revision::commit_contents` for a batch held in memory (no working tree, no staged state); `file::stage` + `revision::commit` for `commit_file_from` and `put_file`;"),
("commit_files (compare-tip atomic byte batch)",
 "commit_files (compare-tip atomic byte batch, committed from memory with no checkout write)"),
("- Working-tree files are an optional checkout (`put_file` / `materialize_to`).",
 "- Working-tree files are an optional checkout (`put_file`, `commit_file_from`, `materialize_to`). A batch commit writes none, and deletes a checkout at a path it changes."),
]
