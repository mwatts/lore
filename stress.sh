#!/bin/bash
# Load the host (CPU and synced writes) while the probe seeds, to slow the projection's close.
D=/home/mwatts/code/github/mwatts/ashlar-r4-lore/.tmp/stress
mkdir -p $D
pids=()
for i in 1 2 3 4 5 6 7 8; do ( while :; do :; done ) & pids+=($!); done
for i in 1 2 3 4; do ( while :; do dd if=/dev/zero of=$D/f$i bs=1M count=256 conv=fsync status=none; done ) & pids+=($!); done
/home/mwatts/code/github/mwatts/r4-scratch/fx-run.sh fx-stress-50000 50000 1 0
kill ${pids[@]}
rm -rf $D
