#!/bin/bash
# Print the probe's headline lines for each measurement file given.
for f in "$@"; do
    echo "== $f"
    grep -E "single creates: |single edits: |bytes written \(write_bytes\)|panicked|after a reopen|seeded in" "/home/mwatts/code/github/mwatts/r4-scratch/measure/$f.txt" | grep -v thread
    grep -E "^\.lore|journal" "/home/mwatts/code/github/mwatts/r4-scratch/measure/$f.txt" | head -8
done
