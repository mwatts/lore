#!/bin/bash
# fx-run.sh <label> <records> [singles] [edits] [shim]: run the current release probe binary.
ASHLAR=/home/mwatts/code/github/mwatts/ashlar-r4-lore
OUT=/home/mwatts/code/github/mwatts/r4-scratch/measure
bin=$(grep -o 'target/release/deps/cairn-[0-9a-f]*' $OUT/fx-build.txt | tail -1)
cd $ASHLAR/crates/ashlar-cairn
pre=""
[ "${5:-}" = shim ] && pre="LD_PRELOAD=/home/mwatts/code/github/mwatts/r4-scratch/shim/r4shim2.so"
uptime > $OUT/$1.txt
env $pre R4FOLDLOG=1 R4_RECORDS=$2 R4_SINGLES=${3:-20} R4_EDITS=${4:-20} TMPDIR=$ASHLAR/.tmp timeout 3000 $ASHLAR/$bin footprint::seeding_footprint_probe --ignored --nocapture >> $OUT/$1.txt 2>&1; echo "exit $?" >> $OUT/$1.txt
uptime >> $OUT/$1.txt
echo finished $1
