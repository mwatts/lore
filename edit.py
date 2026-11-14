"""Apply literal replacements: edit.py SPEC.py, where SPEC defines PATH and EDITS=[(old,new),...]."""
import sys

spec = {}
exec(open(sys.argv[1]).read(), spec)
p = spec['PATH']
s = open(p).read()
for i, edit in enumerate(spec['EDITS']):
    old, new = edit[0], edit[1]
    count = edit[2] if len(edit) > 2 else 1
    n = s.count(old)
    if n == 0:
        sys.exit(f"edit {i}: not found: {old[:120]!r}")
    if count == 1 and n != 1:
        sys.exit(f"edit {i}: found {n} times: {old[:120]!r}")
    s = s.replace(old, new) if count == 0 else s.replace(old, new, count)
open(p, 'w').write(s)
print(f"applied {len(spec['EDITS'])} edits to {p}")
