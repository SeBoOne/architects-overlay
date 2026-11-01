import json, re
out = json.load(open('_prep/remaining.json'))
BAD_EXACT = set(json.load(open("_prep/bad_exact.json")))
CSS_CLASSES = set([
    'deco t1', 'deco t2', 'v sell', 'vC-kpi buy', 'vC-kpi sell', 'vC-kpi margin',
    'vC-c buy', 'vC-c nil', 'vC-c sell', 'val buy', 'val sell', 'psub buy', 'psub sell',
    'ptab locgrp', 'loc muted', 'pr vC-r', 'hchip ins', 'hbtn danger', 'hmod-add hmod-add-meta',
    'cfg-sect cfg-sect-language', 'chip cfg-sc-capture', 'h cfg-g-head', 'chip warn',
    'chip view', 'chip add', 'if-pick free', 'btn-ghost danger', 'btn-ghost btn-danger',
    'btn btn-danger', 'mrow trow', 'mrow rrow', 'r-chip r-chip-age', 'r-chip r-chip-menge',
    'rd-pair-price sell', 'qnum rb-qnum', 'rtext rta-input', 'rtext scb-input', 'md-desc empty',
    'pane-error advert-pane', 'mbar2 advert-desc-row', 'mbar2 advert-foot', 'rtext advert-desc',
])
keep = []
for r in out:
    t = r['text']
    if t in BAD_EXACT or t in CSS_CLASSES:
        continue
    if re.match(r'^(padding|margin|width|display|opacity|position|align|flex|gap|background|color|cursor|max-|min-|z-index|border|text-|overflow|white-space|justify|top|right|left|bottom)[a-z\-: 0-9,()%;.]*$', t):
        continue
    if re.match(r'^M[\d. ]', t):
        continue
    if re.fullmatch(r'[a-z0-9\-]+', t):
        continue
    if re.fullmatch(r'[a-z]+ [a-z0-9\- ]+', t) and not re.search('[äöüß]', t):
        continue
    keep.append(r)
json.dump(keep, open('_prep/remaining-real.json', 'w'), ensure_ascii=False, indent=1)
print(len(keep))
for r in keep:
    print(r['file'].split('/')[-1], r['lines'][:2], repr(r['text'][:95]))
