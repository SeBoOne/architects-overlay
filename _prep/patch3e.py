E = [
 ('src-tauri/src/trade.rs', 'assert_eq!(GROUPS[0].1, "Commodities");\n        assert_eq!(GROUPS[1].1, "Items");\n        assert_eq!(GROUPS[2].1, "Locations");',
  'assert_eq!(GROUPS[0].1, "search.group_commodities");\n        assert_eq!(GROUPS[1].1, "search.group_items");\n        assert_eq!(GROUPS[2].1, "search.group_locations");', 1),
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
