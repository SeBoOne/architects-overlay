E = [
 ('src/lib/Inventory.svelte', "{r.entity_kind === 'item' ? 'Stk' : 'SCU'}",
     "{r.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}", 5),
 ('src/lib/Inventory.svelte', "{m.list[0]?.entity_kind === 'item' ? 'Stk' : 'SCU'}",
     "{m.list[0]?.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}", 1),
 ('src/lib/Inventory.svelte', ' Pos · ', " {$t('inventory.pos_short')} · ", 4),
 ('src/routes/+page.svelte',
  "<div class=\"hangar-empty\">Keine Einträge{hangarFilter !== 'all' ? ` (Filter: ${hangarFilter.toUpperCase()})` : ''} {input.trim() ? ` · Search: ${input.trim()}` : ''} — Import oben.</div>",
  "<div class=\"hangar-empty\">{$t('hangar.no_entries')}{hangarFilter !== 'all' ? ` (${$t('common.filter')}: ${hangarFilter.toUpperCase()})` : ''} {input.trim() ? ` · ${$t('common.search')}: ${input.trim()}` : ''} — {$t('hangar.import_above')}</div>",
  1),
]
fails = []
for f, old, new, cnt in E:
    src = open(f).read()
    n = src.count(old)
    if n != cnt:
        fails.append((f, old[:60], n, cnt)); continue
    open(f, 'w').write(src.replace(old, new))
print('applied', len(E) - len(fails), 'of', len(E))
for x in fails: print('FAIL', x)
