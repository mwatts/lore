#!/bin/bash
# Run the c1 probe binary at 20,000 Records, keep the archive, and count index file kinds.
ASHLAR=/home/mwatts/code/github/mwatts/ashlar-r4-lore
S=/home/mwatts/code/github/mwatts/r4-scratch
bin=$(grep -o 'target/release/deps/cairn-[0-9a-f]*' $S/measure/c1-build.txt | tail -1)
rm -rf $S/keep20k
cd $ASHLAR/crates/ashlar-cairn
R4_RECORDS=20000 R4_KEEP=$S/keep20k TMPDIR=$ASHLAR/.tmp timeout 1200 $ASHLAR/$bin footprint::seeding_footprint_probe --ignored --nocapture > $S/c1-20k.txt 2>&1
grep "bytes written (w" $S/c1-20k.txt
find $S/keep20k/.lore/immutable/index -type f | sed 's#.*/##' | sed 's/[0-9a-f][0-9a-f]$/XX/' | sort | uniq -c | sort -rn | head
find $S/keep20k/.lore/immutable/index -name level | head -300 | xargs -n1 od -An -tu8 2>/dev/null | sort | uniq -c
