import re, sys
path = sys.argv[1]
lines = open(path).read().split('\n')
for l in map(int, sys.argv[2:]):
    f = ''
    for i in range(l - 1, -1, -1):
        m = re.search(r'fn ([a-z_0-9]+)', lines[i])
        if m and not lines[i].strip().startswith('//'):
            f = m.group(1)
            break
    print(l, f, '|', lines[l - 1].strip()[:90])
