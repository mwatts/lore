#!/bin/bash
# Move the Cairn pin commit to a new Lore hash, keeping the dev patch in the working tree.
set -e
C=/home/mwatts/code/github/mwatts/ashlar/vendor/cairn/.claude/worktrees/agent-a73219e9c3799f25c
S=/home/mwatts/code/github/mwatts/r4-scratch
OLD=$1
NEW=$2
cd $C
cp Cargo.toml $S/cairn-toml-dev
cp Cargo.lock $S/cairn-lock-dev
git show HEAD:Cargo.toml | sed "s/$OLD/$NEW/g" > Cargo.toml
git show HEAD:Cargo.lock | sed "s/$OLD/$NEW/g" > Cargo.lock
sed -i "s/$OLD/$NEW/g; s/\`${OLD:0:8}\` lore-storage/\`${NEW:0:8}\` lore-storage/" docs/lore-embed.md
git add Cargo.toml Cargo.lock docs/lore-embed.md
git commit -q --amend --no-edit
sed "s/$OLD/$NEW/g" $S/cairn-toml-dev > Cargo.toml
cp $S/cairn-lock-dev Cargo.lock
git log --oneline -1
grep -c "$NEW" Cargo.lock docs/lore-embed.md
