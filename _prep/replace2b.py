import json, re, sys
sys.path.insert(0, '_prep')
from replace2 import M, FILES, regions  # reuse table (module import runs its body again — guard below)

# replace2 already ran; here we finish multi-occurrence texts.
report = {'script': 0, 'markup': 0, 'leftover': []}
new_en = {}
new_de = {}
for f in FILES:
    src = open(f).read()
    s0, e0 = regions(src)
    prot = []
    for m in re.finditer(r'<!--[\s\S]*?-->', src):
        if not (m.start() >= s0 and m.end() <= e0):
            prot.append((m.start(), m.end()))
    changed = True
    while changed:
        changed = False
        for text, (key, en, de) in M.items():
            for q in ("'", '"'):
                lit = q + text + q
                i = 0
                while True:
                    i = src.find(lit, i)
                    if i < 0:
                        break
                    if any(a <= i < b for a, b in prot):
                        i += 1
                        continue
                    in_script = s0 <= i < e0
                    new = ("t('%s')" if in_script else "$t('%s')") % key
                    src = src[:i] + new + src[i + len(lit):]
                    e0 = src.find('</script>')  # region end unchanged in length terms; recompute anyway
                    s0 = src.find('<script')
                    changed = True
                    new_en[key] = en
                    new_de[key] = de
                    if in_script:
                        report['script'] += 1
                    else:
                        report['markup'] += 1
                    i += len(new)
                    break  # recompute protections after each edit
    # leftover check
    for text in M:
        for q in ("'", '"'):
            lit = q + text + q
            i = 0
            while True:
                i = src.find(lit, i)
                if i < 0:
                    break
                if not (s0 <= i < e0) and not any(a <= i < b for a, b in prot):
                    report['leftover'].append((f, text))
                i += 1
    open(f, 'w').write(src)

print('script', report['script'], 'markup', report['markup'], 'leftover', len(set(report['leftover'])))
for l in sorted(set(report['leftover'])):
    print('LEFT', l)
w = json.load(open('_prep/wave2-strings.json'))
w['new_en'].update(new_en)
w['new_de'].update(new_de)
json.dump(w, open('_prep/wave2-strings.json', 'w'), ensure_ascii=False, indent=1)
