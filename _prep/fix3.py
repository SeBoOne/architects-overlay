import json, re

# 1) ariaLabel=$t('x')  ->  ariaLabel={$t('x')}
FILES = ['src/routes/+page.svelte', 'src/lib/Inventory.svelte', 'src/lib/Rail.svelte', 'src/lib/SelectCombo.svelte', 'src/lib/RoutesTypeahead.svelte']
for f in FILES:
    src = open(f).read()
    n = 0
    def rep(m):
        global n
        n += 1
        return m.group(1) + "={$t('" + m.group(2) + "')}"
    src = re.sub(r"(\w+)=\$t\('([^']+)'\)", rep, src)
    open(f, 'w').write(src)
    print(f, 'attrfix', n)

# 2) script-region bare t('...') -> tr('...')
def regions(src):
    s = src.find('<script')
    e = src.find('</script>')
    return s, e

for f in FILES:
    src = open(f).read()
    s0, e0 = regions(src)
    head, script, tail = src[:s0], src[s0:e0], src[e0:]
    before = len(re.findall(r"(?<![\w.$])t\('[a-z0-9_.]+'\)", script))
    script = re.sub(r"(?<![\w.$])t\('([a-z0-9_.]+)'\)", r"tr('\1')", script)
    src = head + script + tail
    # add tr to the i18n import
    m = re.search(r"import \{([^}]*)\} from '[^']*i18n';", src)
    if m and 'tr' not in m.group(1).split(','):
        names = m.group(1)
        src = src[:m.start()] + "import {" + names + ", tr } from '" + src[m.start():m.end()].split(' from ')[1] if False else src
        # simpler: rebuild
        import re as _re
        mm = _re.search(r"import \{([^}]*)\} from ('[^']*i18n')", src)
        names = mm.group(1).rstrip().rstrip(',')
        src = src[:mm.start()] + "import { " + names + ", tr } from " + mm.group(2) + src[mm.end():]
    open(f, 'w').write(src)
    print(f, 'scriptcalls', before)
