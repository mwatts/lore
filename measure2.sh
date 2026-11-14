#!/bin/bash
# Final change-1 measurements: build once, then 20/20 and 300-create runs at 5k and 50k.
ASHLAR=/home/mwatts/code/github/mwatts/ashlar-r4-lore
S=/home/mwatts/code/github/mwatts/r4-scratch
OUT=$S/measure
cd $ASHLAR
grep -n "^lore" Cargo.toml | tail -2 > $OUT/c1f-patch.txt
cargo test --release -p ashlar-cairn --test cairn --no-run > $OUT/c1f-build.txt 2>&1 || { echo build failed; exit 1; }
bin=$(grep -o 'target/release/deps/cairn-[0-9a-f]*' $OUT/c1f-build.txt | tail -1)
cd crates/ashlar-cairn
for records in 5000 50000; do
    R4_RECORDS=$records R4_SINGLES=20 R4_EDITS=20 TMPDIR=$ASHLAR/.tmp timeout 3000 $ASHLAR/$bin footprint::seeding_footprint_probe --ignored --nocapture > $OUT/c1f-$records.txt 2>&1
    echo "c1f $records done"
    R4_RECORDS=$records R4_SINGLES=300 TMPDIR=$ASHLAR/.tmp timeout 3000 $ASHLAR/$bin footprint::seeding_footprint_probe --ignored --nocapture > $OUT/c1f-$records-300.txt 2>&1
    echo "c1f $records 300 done"
done
