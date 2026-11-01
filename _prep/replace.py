import json, re, sys
sys.path.insert(0,'_prep')
from keymap import K
rows=json.load(open('_prep/unique-cands.json'))
texts=[(K[i], rows[i-1]['text']) for i in range(1,len(rows)+1)]
ATTRS=['title','aria-label','placeholder','alt','label']
report={'attr':0,'text':0,'unresolved':[]}
FILES=['src/routes/+page.svelte','src/lib/Inventory.svelte','src/lib/Rail.svelte','src/lib/RoutesTypeahead.svelte','src/lib/SelectCombo.svelte','src/routes/+layout.svelte']
for f in FILES:
    src=open(f).read()
    prot=[]
    for m in re.finditer(r'<script[^>]*>[\s\S]*?</script>|<style[^>]*>[\s\S]*?</style>', src):
        prot.append((m.start(),m.end()))
    comments=[(m.start(),m.end()) for m in re.finditer(r'<!--[\s\S]*?-->', src)]
    def blocked(pos):
        return any(a<=pos<b for a,b in prot) or any(a<=pos<b for a,b in comments)
    edits=[]
    used=set()
    for key,t in texts:
        tt=re.escape(t)
        found=False
        for a in ATTRS:
            for pat in (a+'="'+tt+'"', a+"='"+tt+"'"):
                for m in re.finditer(pat, src):
                    if not blocked(m.start()):
                        edits.append((m.start(), m.end(), f"{a}={{$t('{key}')}}"))
                        found=True; used.add((t,'attr'))
        # markup text node  > t <
        pat='>\\s*'+tt+r'\s*<'
        for m in re.finditer(pat, src):
            if not blocked(m.start()):
                edits.append((m.start(), m.end(), ">{{$t('%s')}}<"%key))
                found=True; used.add((t,'text'))
        if not found:
            report['unresolved'].append({'text':t,'key':key,'file':f})
    # prefer longer matches on overlap: sort by start, then longest first
    edits.sort(key=lambda e:(e[0], -(e[1]-e[0])))
    kept=[]; last_end=-1
    for s,e,r in edits:
        if s < last_end:
            continue
        kept.append((s,e,r)); last_end=e
    edits=kept
    out=[]; idx=0
    for s,e,r in edits:
        out.append(src[idx:s]); out.append(r); idx=e
    out.append(src[idx:])
    open(f+'.bak','w').write(src)
    open(f,'w').write(''.join(out))
    report['attr']+=sum(1 for s,e,r in edits if '=$t(' in r)
    report['text']+=sum(1 for s,e,r in edits if r.startswith('>{') or r.startswith('>\n{') or r.startswith('> ') is False and r.startswith('>'))
json.dump(report, open('_prep/replace-report.json','w'), ensure_ascii=False, indent=1)
print('attr',report['attr'],'text',report['text'],'unresolved',len(report['unresolved']))
for u in report['unresolved'][:80]:
    print('UNRESOLVED', u['key'], repr(u['text'][:70]))
