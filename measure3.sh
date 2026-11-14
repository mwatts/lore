#!/bin/bash
# Final change-1 (bulk-flush) measurements at 5k and 50k, 20 creates and 20 edits.
ASHLAR=/home/mwatts/code/github/mwatts/ashlar-r4-lore
OUT=/home/mwatts/code/github/mwatts/r4-scratch/measure
cd $ASHLAR
cargo test --release -p ashlar-cairn --test cairn --no-run > $OUT/c1g-build.txt 2>&1 || { echo build failed; exit 1; }
bin=$(grep -o 'target/release/deps/cairn-[0-9a-f]*' $OUT/c1g-build.txt | tail -1)
cd crates/ashlar-cairn
for records in 5000 50000; do
    R4_RECORDS=$records R4_SINGLES=20 R4_EDITS=20 TMPDIR=$ASHLAR/.tmp timeout 3000 $ASHLAR/$bin footprint::seeding_footprint_probe --ignored --nocapture > $OUT/c1g-$records.txt 2>&1
    echo "c1g $records done"
done
