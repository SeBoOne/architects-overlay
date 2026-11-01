import json, sys

# (file, old, new, expected_count)
E = []

# ---- +page.svelte ----------------------------------------------------------
F = 'src/routes/+page.svelte'
E += [
 (F, "{ id: 'hangar', label: 'Hangar' },\n\t\t{ id: 'routes', label: 'Handelsrouten' },\n\t\t{ id: 'inventory', label: 'Inventar' }",
     "{ id: 'hangar', label: 'nav.hangar' },\n\t\t{ id: 'routes', label: 'nav.routes_full' },\n\t\t{ id: 'inventory', label: 'nav.inventory' }", 1),
 (F, "<div class=\"cfg-sc-lbl\">{f.label}</div>",
     "<div class=\"cfg-sc-lbl\">{$t(f.label)}</div>", 2),
 (F, "„{modeHotkeyFields.find((f) => f.id === hkCapture)?.label}“",
     "„{modeHotkeyFields.find((f) => f.id === hkCapture)?.label ? $t(modeHotkeyFields.find((f) => f.id === hkCapture)!.label) : ''}“", 1),
 (F, "{ id: 'toggle' as ShortcutField, label: 'Overlay-Toggle',",
     "{ id: 'toggle' as ShortcutField, label: 'settings.shortcut_toggle',", 1),
 (F, "{ id: 'click' as ShortcutField, label: 'Click-through',",
     "{ id: 'click' as ShortcutField, label: 'settings.shortcut_click',", 1),
 # INSURANCE_CHOICES labels -> keys (values unchanged, DB-safe)
 (F, "{ value: '', label: 'Ohne' },\n\t\t{ value: '3 Monate', label: '3 Monate' },\n\t\t{ value: '6 Monate', label: '6 Monate' },\n\t\t{ value: '12 Monate', label: '12 Monate' },\n\t\t{ value: '24 Monate', label: '24 Monate' },\n\t\t{ value: '120 Monate', label: '120 Monate' },\n\t\t{ value: 'LTI', label: 'LTI' }",
     "{ value: '', label: 'common.none' },\n\t\t{ value: '3 Monate', label: 'hangar.ins_3m' },\n\t\t{ value: '6 Monate', label: 'hangar.ins_6m' },\n\t\t{ value: '12 Monate', label: 'hangar.ins_12m' },\n\t\t{ value: '24 Monate', label: 'hangar.ins_24m' },\n\t\t{ value: '120 Monate', label: 'hangar.ins_120m' },\n\t\t{ value: 'LTI', label: 'hangar.ins_lti' }", 1),
 (F, "options={INSURANCE_CHOICES}",
     "options={INSURANCE_CHOICES.map((c) => ({ ...c, label: $t(c.label) }))}", 2),
 (F, "<b>{$t('hangar.insurance')}</b><span>{hangarFormIns || 'Ohne'}</span>",
     "<b>{$t('hangar.insurance')}</b><span>{hangarFormIns || $t('common.none')}</span>", 1),
 # origin / type option labels (inline in markup -> reactive)
 (F, "options={[{ value: 'pledge', label: 'Pledged' }, { value: 'ingame', label: 'In-Game' }]}",
     "options={[{ value: 'pledge', label: $t('hangar.origin_pledged') }, { value: 'ingame', label: $t('hangar.origin_ingame') }]}", 1),
 (F, "options={[{ value: 'ingame', label: 'In-Game' }, { value: 'pledge', label: 'Pledged' }]}",
     "options={[{ value: 'ingame', label: $t('hangar.origin_ingame') }, { value: 'pledge', label: $t('hangar.origin_pledged') }]}", 1),
 (F, "options={[{ value: 'ship', label: 'Schiff' }, { value: 'vehicle', label: 'Fahrzeug' }]}",
     "options={[{ value: 'ship', label: $t('common.ship') }, { value: 'vehicle', label: $t('common.vehicle') }]}", 1),
 # ariaLabels
 (F, 'ariaLabel="Modul-Herkunft"', "ariaLabel={$t('hangar.module_origin_aria')}", 1),
 (F, 'ariaLabel="Modul-Versicherung"', "ariaLabel={$t('hangar.module_insurance_aria')}", 1),
 (F, 'ariaLabel="Schiff/Fahrzeug-Name"', "ariaLabel={$t('hangar.name_aria')}", 1),
 (F, 'ariaLabel="Typ"', "ariaLabel={$t('hangar.type')}", 1),
 (F, 'ariaLabel="Herkunft"', "ariaLabel={$t('hangar.origin')}", 1),
 (F, 'ariaLabel="Versicherung"', "ariaLabel={$t('hangar.insurance')}", 1),
 (F, 'ariaLabel="Basisschiff"', "ariaLabel={$t('hangar.base_ship')}", 1),
 (F, 'ariaLabel="Listing Type"', "ariaLabel={$t('shop.listing_type')}", 1),
 (F, 'ariaLabel="Currency"', "ariaLabel={$t('shop.currency')}", 1),
 (F, 'ariaLabel="Einheit"', "ariaLabel={$t('shop.unit')}", 1),
 (F, 'ariaLabel="Kategorie"', "ariaLabel={$t('shop.category')}", 1),
 (F, 'ariaLabel="Laufzeit"', "ariaLabel={$t('shop.duration')}", 1),
 (F, 'ariaLabel="Item Source"', "ariaLabel={$t('shop.item_source')}", 1),
 (F, 'ariaLabel="Availability"', "ariaLabel={$t('shop.availability')}", 1),
 # listing-type options
 (F, "{ value: 'item', label: 'Item' },", "{ value: 'item', label: $t('shop.item') },", 1),
 (F, "{ value: 'service', label: 'Service' },", "{ value: 'service', label: $t('shop.opt_service') },", 1),
 (F, "{ value: 'contract', label: 'Contract' }", "{ value: 'contract', label: $t('shop.opt_contract') }", 1),
 (F, "{ value: '', label: '— bitte wählen —' },", "{ value: '', label: $t('common.choose_placeholder') },", 1),
 (F, "{ value: '', label: '— (optional) —' },", "{ value: '', label: $t('common.ph_optional') },", 2),
 # hidden toggle Ja/No
 (F, ">{advertForm.hidden ? 'Ja' : 'No'}</button>", ">{advertForm.hidden ? $t('common.yes') : $t('common.no')}</button>", 1),
 # confirm-chip labels
 (F, "advertForm.operation === 'sell' ? 'Verkaufs-Angebot' : 'Kauf-Gesuch'",
     "advertForm.operation === 'sell' ? $t('shop.sell_offer_label') : $t('shop.buy_request_label')", 1),
 # shopNote templates
 (F, "\t\t\tshopNote = (f.operation === 'sell'\n\t\t\t\t? 'Listing „' + f.title.trim() + '“ wurde angelegt.'\n\t\t\t\t: 'Kauf-Gesuch „' + f.title.trim() + '“ wurde angelegt.') +\n\t\t\t\t' Es erscheint nach UEX-Approval in deiner Offers-Liste.';",
     "\t\t\tshopNote =\n\t\t\t\ttr(f.operation === 'sell' ? 'shop.created_note' : 'shop.request_created_note', {\n\t\t\t\t\ttitle: f.title.trim()\n\t\t\t\t}) + ' ' + tr('shop.appears_after_approval');", 1),
 # market loading more
 (F, "{mLoadingMore ? 'lädt …' : 'automatisch nachladen …'}",
     "{mLoadingMore ? $t('market.loading_more') : $t('market.autoload_more')}", 1),
 # price-detail heads
 (F, "{dIsTerminal ? 'Artikel' : 'Terminal'}</div>", "{dIsTerminal ? $t('market.n_items') : $t('market.n_terminals')}</div>", 1),
 (F, "<span>{dIsTerminal ? 'Item' : 'Terminal'}</span>", "<span>{dIsTerminal ? $t('shop.item') : $t('market.n_terminals')}</span>", 1),
 # trade status chips
 (F, "tradesDeal.date_closed && tradesDeal.date_closed_client ? 'Abgeschlossen' : 'Beendet'",
     "tradesDeal.date_closed && tradesDeal.date_closed_client ? $t('trades.status_closed') : $t('trades.status_ended')", 1),
 # lightbox alt + caption
 (F, "alt={'Bild ' + (pi + 1) + ' von „' + offerDetail!.title + '“'}",
     "alt={$t('market.image_n_of', { n: pi + 1, title: offerDetail!.title })}", 1),
 (F, "alt={'Vergrößert: ' + lightbox.title}", "alt={$t('market.zoomed_alt', { title: lightbox.title })}", 1),
 (F, "{lightbox.title} — Klick außerhalb oder Esc schließt",
     "{lightbox.title} — {$t('market.lightbox_close_hint')}", 1),
 # shopStatus -> keys
 (F, "\t\tif (!o.approved) return tr('shop.status_pending');\n\t\tif (!o.expires) return 'aktiv';\n\t\tif (o.expires <= Math.floor(Date.now() / 1000)) return 'abgelaufen';\n\t\treturn 'läuft ab ' + new Date(o.expires * 1000).toLocaleDateString('de-DE');",
     "\t\tif (!o.approved) return tr('shop.status_pending');\n\t\tif (!o.expires) return tr('shop.status_active');\n\t\tif (o.expires <= Math.floor(Date.now() / 1000)) return tr('shop.status_expired');\n\t\treturn tr('shop.status_expires_on', { date: new Date(o.expires * 1000).toLocaleDateString('de-DE') });", 1),
 # two long empty-state notes
 (F, "'Noch keine Commodity-Trades erfasst — lege deinen ersten Trade an („+ Trade erfassen“) oder führe ihn direkt auf UEX.'",
     "tr('trades.no_commodity_trades')", 1),
 (F, "'Das Listing taucht seit über 24 h nicht in der UEX-API auf — Freigabe unbestätigt (evtl. abgelehnt). Prüfe es im Listings-Manager.'",
     "tr('shop.approval_stale_note')", 1),
 (F, "'UEX hat dieses Listing noch nicht freigegeben — es erscheint öffentlich erst nach der Moderation.'",
     "tr('shop.pending_moderation_title')", 1),
 # total-value title with interpolation
 (F, 'title="Gesamtwert: {fmt(trk.scu)} SCU × {fmtPrice(trk.price)} aUEC"',
     "title={$t('trades.total_value_title', { scu: fmt(trk.scu), price: fmtPrice(trk.price) })}", 1),
 # group labels from Rust are keys now
 (F, "<div class=\"vgroup\">{g.label} <span class=\"n\">{g.hits.length}</span></div>",
     "<div class=\"vgroup\">{$t(g.label)} <span class=\"n\">{g.hits.length}</span></div>", 1),
 (F, "<div class=\"vgroup\">{g.label} <span class=\"n\">0</span></div>",
     "<div class=\"vgroup\">{$t(g.label)} <span class=\"n\">0</span></div>", 1),
]

# hangar-empty line: fetch exact text first? handle generically below.
# ---- Inventory.svelte ------------------------------------------------------
G = 'src/lib/Inventory.svelte'
E += [
 (G, "{fEntityKind === 'item' ? 'Item-Entity' : 'Commodity-Entity'}",
     "{fEntityKind === 'item' ? $t('inventory.item_entity') : $t('inventory.commodity_entity')}", 1),
 (G, "{editingId === null ? 'Erfassen' : 'Speichern'}",
     "{editingId === null ? $t('inventory.record_btn') : $t('common.save')}", 1),
 (G, "{r.entity_kind === null ? 'Freitext' : r.entity_kind === 'item' ? 'Item' : 'Commodity'}",
     "{r.entity_kind === null ? $t('inventory.free_text') : r.entity_kind === 'item' ? $t('shop.item') : $t('common.commodity')}", 1),
 (G, "{r.entity_kind === 'item' ? 'Stk' : 'SCU'}",
     "{r.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}", 1),
 (G, "{filter.trim() ? `Keine Positionen passen zu „${filter}“.` : 'Inventar leer — erfasse die erste Position mit <b>N</b>.'}",
     "{filter.trim() ? $t('inventory.no_match_filter', { filter }) : ''}\n\t\t\t\t\t{@html filter.trim() ? '' : $t('inventory.empty_note')}", 1),
]

# ---- Rail.svelte -----------------------------------------------------------
H = 'src/lib/Rail.svelte'
E += [
 (H, "{ id: 'search', key: 'F1', label: 'Suche', title: 'Suche (F1)', icon: 'search' },",
     "{ id: 'search', key: 'F1', label: 'nav.search', title: 'nav.search_title', icon: 'search' },", 1),
 (H, "{ id: 'market', key: 'F2', label: 'Marktplatz', title: 'Marktplatz (F2)', icon: 'market' },",
     "{ id: 'market', key: 'F2', label: 'nav.marketplace', title: 'nav.marketplace_title', icon: 'market' },", 1),
 (H, "{ id: 'routes', key: 'F3', label: 'Routen', title: 'Handelsrouten (F3)', icon: 'routes' },",
     "{ id: 'routes', key: 'F3', label: 'nav.routes', title: 'nav.routes_title', icon: 'routes' },", 1),
 (H, "{ id: 'inventory', key: 'F4', label: 'Inventar', title: 'Inventar (F4)', icon: 'inventory' },",
     "{ id: 'inventory', key: 'F4', label: 'nav.inventory', title: 'nav.inventory_title', icon: 'inventory' },", 1),
 (H, "{ id: 'hangar', key: 'F11', label: 'Hangar', title: 'Hangar (F11)', icon: 'hangar' },",
     "{ id: 'hangar', key: 'F11', label: 'nav.hangar', title: 'nav.hangar_title', icon: 'hangar' },", 1),
 (H, "{ id: 'settings', key: 'F12', label: 'Einstellungen', title: 'Einstellungen (F12)', icon: 'settings' }",
     "{ id: 'settings', key: 'F12', label: 'nav.settings', title: 'nav.settings_title', icon: 'settings' }", 1),
]

# apply
fails = []
for f, old, new, cnt in E:
    src = open(f).read()
    n = src.count(old)
    if n != cnt:
        fails.append((f, old[:60], n, cnt))
        continue
    src = src.replace(old, new)
    open(f, 'w').write(src)
print('applied', len(E) - len(fails), 'of', len(E))
for x in fails:
    print('FAIL', x)
json.dump(fails, open('_prep/wave3-fails.json', 'w'), ensure_ascii=False)
