#!/bin/bash
# Headline lines of the final change-1 runs.
for f in "$@"; do
    echo "== $f"
    grep -E "single creates: |single edits: |bytes written \(w|reopen wrote|panicked" /home/mwatts/code/github/mwatts/r4-scratch/measure/$f.txt | grep -v thread
done
