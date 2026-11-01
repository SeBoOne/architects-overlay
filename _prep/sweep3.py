import re, json
FILES = ['src/routes/+page.svelte', 'src/lib/Inventory.svelte', 'src/lib/Rail.svelte', 'src/lib/SelectCombo.svelte', 'src/lib/RoutesTypeahead.svelte']
seen = {}
for f in FILES:
    src = open(f).read()
    s0 = src.find('<script')
    e0 = src.find('</script>')
    markup = src[:s0] + src[e0:]
    markup = re.sub(r'<!--[\s\S]*?-->', '', markup)
    for m in re.finditer(r"'([^'\n]+)'", markup):
        t = m.group(1)
        if '$t(' in t or t.strip().startswith('$t'):
            continue
        if not re.search(r'[A-Za-zÄÖÜäöüß]', t):
            continue
        if re.fullmatch(r'[a-z0-9_\-. ]+', t):
            continue  # classes/ids/attrs
        if re.fullmatch(r'[A-Z0-9_\-+/ ]+', t):
            continue  # enum-ish values
        pos = markup.find(t)
        ln = src[:src.find(t) if src.find(t) > 0 else 0].count('\n') + 1
        seen.setdefault(t, []).append((f, ln))
for t, occ in sorted(seen.items()):
    print(repr(t), occ[:3])
print('total', len(seen))
