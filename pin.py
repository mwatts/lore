import re
root = '/home/mwatts/code/github/mwatts/ashlar/vendor/cairn/.claude/worktrees/agent-a73219e9c3799f25c/'
old = '24545866cf1a8f5ae3f0a996270e7aa7cdd868ac'
new = 'fb9e3f45278a30f0183c3bd1922fbb2caffc47ff'
p = root + 'Cargo.toml'
s = open(p).read()
a = '''# Fork commit: EpicGames/lore 5fa7453f plus the macOS getattrlist name lookup.
# The upstream pull request branch is fix/macos-exact-name-lookup on current main.'''
assert a in s
s = s.replace(a, '''# Fork commit: EpicGames/lore 5fa7453f plus the macOS getattrlist name lookup,
# plus Ashlar R4's commit-from-memory and store journal (branch `ashlar`).
# The upstream pull request branch is fix/macos-exact-name-lookup on current main.''', 1)
assert s.count(old) == 2
s = s.replace(old, new)
open(p, 'w').write(s)
p = root + 'Cargo.lock'
s = open(p).read()
n = s.count(old)
s = s.replace(old, new)
open(p, 'w').write(s)
print('lock replacements', n)
