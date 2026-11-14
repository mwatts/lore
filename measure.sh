#!/bin/bash
# Measure each Lore configuration with the R4 probe, one at a time.
set -u
ASHLAR=/home/mwatts/code/github/mwatts/ashlar-r4-lore
OUT=/home/mwatts/code/github/mwatts/r4-scratch/measure
mkdir -p "$OUT"
cd "$ASHLAR"
for config in "$@"; do
    dir=$config
    [ "$config" = c1 ] && dir=lore || dir=lore-$config
    sed -i -E "s#/home/mwatts/code/github/mwatts/lore[^/]*/lore\"#/home/mwatts/code/github/mwatts/$dir/lore\"#; s#/home/mwatts/code/github/mwatts/lore[^/]*/lore-base\"#/home/mwatts/code/github/mwatts/$dir/lore-base\"#" Cargo.toml
    grep -n "^lore" Cargo.toml | tail -2 > "$OUT/$config-patch.txt"
    cargo test --release -p ashlar-cairn --test cairn --no-run > "$OUT/$config-build.txt" 2>&1 || { echo "$config build failed"; continue; }
    bin=$(grep -o 'target/release/deps/cairn-[0-9a-f]*' "$OUT/$config-build.txt" | tail -1)
    for records in 5000 50000; do
        (cd crates/ashlar-cairn && R4_RECORDS=$records R4_SINGLES=20 R4_EDITS=20 TMPDIR=$ASHLAR/.tmp timeout 3000 "$ASHLAR/$bin" footprint::seeding_footprint_probe --ignored --nocapture > "$OUT/$config-$records.txt" 2>&1)
        echo "$config $records done"
    done
done
