E = [
 ('src/lib/Rail.svelte', "\tfunction titleFor(en: Entry): string {\n\t\tconst label = en.id === 'routes' ? 'Handelsrouten' : en.label;\n\t\treturn `${label} (${keyFor(en.id)})`;\n\t}",
     "\tfunction titleFor(en: Entry): string {\n\t\tconst label = en.id === 'routes' ? 'nav.routes_full' : en.label;\n\t\treturn `${tr(label)} (${keyFor(en.id)})`;\n\t}", 1),
 ('src/lib/Rail.svelte', '<span class="rlbl">{en.label}</span>', '<span class="rlbl">{$t(en.label)}</span>', 1),
 ('src-tauri/src/trade.rs', 'const GROUPS: &[(&str, &str, &[EntityKind])] = &[\n    ("commodity", "Commodities", &[EntityKind::Commodities]),\n    ("item", "Items", &[EntityKind::Items]),\n    (\n        "location",\n        "Locations",\n        &[EntityKind::Locations, EntityKind::Terminals],\n    ),\n];',
     'const GROUPS: &[(&str, &str, &[EntityKind])] = &[\n    // T2 (t_c5904bcb): label ist ein i18n-KEY — das Frontend übersetzt\n    // via $t(label); Fallback-Kette liefert en bei fehlender Sprache.\n    (\n        "commodity",\n        "search.group_commodities",\n        &[EntityKind::Commodities]\n    ),\n    ("item", "search.group_items", &[EntityKind::Items]),\n    (\n        "location",\n        "search.group_locations",\n        &[EntityKind::Locations, EntityKind::Terminals]\n    ),\n];', 1),
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
