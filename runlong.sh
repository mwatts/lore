#!/bin/bash
# Run the c1 probe binary with 300 creates, so the journal folds during the phase.
ASHLAR=/home/mwatts/code/github/mwatts/ashlar-r4-lore
S=/home/mwatts/code/github/mwatts/r4-scratch
bin=$(grep -o 'target/release/deps/cairn-[0-9a-f]*' $S/measure/c1-build.txt | tail -1)
cd $ASHLAR/crates/ashlar-cairn
for records in "$@"; do
    R4_RECORDS=$records R4_SINGLES=300 TMPDIR=$ASHLAR/.tmp timeout 3000 $ASHLAR/$bin footprint::seeding_footprint_probe --ignored --nocapture > $S/measure/c1-$records-300.txt 2>&1
    grep -E "single creates: |kept" $S/measure/c1-$records-300.txt | grep -v thread
done
