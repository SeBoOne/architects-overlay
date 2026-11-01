import json

E = [
 ('src/routes/+page.svelte', "{dIsTerminal ? 'Artikel' : 'Terminal'}", "{dIsTerminal ? $t('market.n_items') : $t('market.n_terminals')}", 2),
 ('src/routes/+page.svelte', "{dIsTerminal ? 'Item' : 'Terminal'}", "{dIsTerminal ? $t('shop.item') : $t('market.n_terminals')}", 3),
 ('src/lib/Inventory.svelte', "{r.entity_kind === null ? 'Freitext' : r.entity_kind === 'item' ? 'Item' : 'Commodity'}",
     "{r.entity_kind === null ? $t('inventory.free_text') : r.entity_kind === 'item' ? $t('shop.item') : $t('common.commodity')}", 4),
 ('src/lib/Inventory.svelte', "{#if r.price} · Kauf {fmtPrice(r.price)} aUEC{/if}",
     "{#if r.price} {$t('inventory.purchase_price', { price: fmtPrice(r.price) })}{/if}", 3),
 ('src/lib/Inventory.svelte', "{#if r.price} · Purchase {fmtPrice(r.price)} aUEC{/if}",
     "{#if r.price} {$t('inventory.purchase_price', { price: fmtPrice(r.price) })}{/if}", 1),
 ('src/lib/Inventory.svelte', "{r.entity_kind === 'item' ? 'Stk' : 'SCU'}",
     "{r.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}", 4),
 ('src/lib/Inventory.svelte', "{m.list[0]?.entity_kind === 'item' ? 'Stk' : 'SCU'}",
     "{m.list[0]?.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}", 2),
 ('src/lib/Inventory.svelte', "{g.entries[0]?.entity_kind === 'item' ? 'Stk' : 'SCU'}",
     "{g.entries[0]?.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}", 2),
 ('src/lib/Inventory.svelte', ' Pos · ', " {$t('inventory.pos_short')} · ", 3),
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
