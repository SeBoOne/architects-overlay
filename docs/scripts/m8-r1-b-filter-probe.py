#!/usr/bin/env python3
"""M8-R1-B probe (t_13ba04ff) — Filter-Kategorisierung Crafting (F6), self-contained.

Gates:
  1. „Alle“ bleibt PILL/BUTTON (cchip), ist Default-aktiv und dient als
     Filter-Reset (Ober + Unter zurück auf all)
  2. Oberkategorie = CUSTOM-Dropdown (SelectCombo, KEIN natives <select>):
     Options All/FPS/Ship/Mission Items — Maus-Klick auf Eintrag WIRKLICH
     bedienbar (Regressionsfall Fokusduell-Bug, M7-R1)
  3. Unterkategorie-Dropdown zeigt NUR Optionen der gewählten Oberkategorie
     (Ship → 11 Unterkategorien; FPS → Armor/Weapons/Magazine)
  4. Unterkategorie-Filter greift (z. B. Shield → nur Shield-Items)
  5. Oberkategorie-Wechsel resettet die Unterkategorie
  6. Mission Items: KEINE Unterkategorie (Dropdown ausgeblendet, beides in einem)
  7. Esc schließt das offene Dropdown; Keyboard (Pfeile + Enter) wählt
  8. Kombination mit Hauptsuchfeld funktioniert; Liste startet VOLL (LEER = all)
  9. null/unmapped category → „Sonstige“-Ober nur wenn vorhanden (datengetrieben)

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_13ba04ff")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m8r1b-filter", ignore_errors=True)


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS {name}")
    else:
        FAIL += 1
        print(f"  FAIL {name}  {detail}")


WRAP = r"""
(function () {
  // Realistische Strata-Kategorien (M8-R1-B Mapping-Beleg): alle 3 Ober-
  // kategorien + ein null-category-Item (Sonstige-Fallback).
  const CATS = ['FPS Weapon', 'FPS Armor', 'FPS Magazine', 'Shield', 'Ship Weapon', 'Mission Item'];
  const mk = (i) => ({
    id: 'item-' + i,
    name: 'Item ' + String(i).padStart(3, '0'),
    slug: 'item-' + i,
    category: i === 999 ? null : CATS[i % CATS.length]
  });
  const ITEMS = {
    items: Array.from({ length: 12 }, (_, i) => mk(i)).concat([
      { id: 'item-shield-a', name: 'Milspec Shield A', slug: 'milspec-shield-a', category: 'Shield' },
      { id: 'item-shield-b', name: 'Bulwark Shield B', slug: 'bulwark-shield-b', category: 'Shield' },
      { id: 'item-nullcat', name: 'Mystery Item', slug: 'mystery-item', category: null }
    ]),
    count: 15, attribution: 'Data from celd.space / Strata Mining Tools'
  };
  window.__STRATA_CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_items') { window.__STRATA_CALLS.push({ cmd }); return ITEMS; }
    return orig(cmd, args);
  };
  window.craftRows = () => [...document.querySelectorAll('.crafting .crow')].map(
    (r) => r.querySelector('.crow-name')?.textContent.trim());
  window.craftPills = () => [...document.querySelectorAll('.crafting .cchip')].map((b) => b.textContent.trim());
  window.oberInput = () => document.querySelector('.crafting .cfchips .scb .scb-input');
  window.oberDDItems = () => [...document.querySelectorAll('.crafting .cfchips .scb .scb-item')].map((b) => b.textContent.trim());
  window.oberOpen = () => !!document.querySelector('.crafting .cfchips .scb .scb-dd');
  window.allScbs = () => [...document.querySelectorAll('.crafting .cfchips .scb')];
  window.subInput = () => window.allScbs()[1]?.querySelector('.scb-input') || null;
  window.subDDItems = () => [...(window.allScbs()[1]?.querySelectorAll('.scb-item') || [])].map((b) => b.textContent.trim());
  window.subOpen = () => !!(window.allScbs()[1]?.querySelector('.scb-dd'));
  window.pressKey = (key) => window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  window.setSearch = (v) => {
    const inp = document.querySelector('.q');
    inp.value = v;
    inp.dispatchEvent(new Event('input', { bubbles: true }));
  };
})();
"""


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8797", "--bind", "127.0.0.1",
        "--directory", str(ROOT / "build"),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    for _ in range(60):
        try:
            with urllib.request.urlopen(URL, timeout=1) as r:
                if r.status == 200:
                    break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    else:
        raise RuntimeError("static server did not come up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1600,1000", "--hide-scrollbars",
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-m8r1b-filter",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9234/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        raise RuntimeError("CDP page target did not come up")

    async with websockets.connect(ws, max_size=None) as cdp:
        mid = {"n": 0}
        pending = {}

        async def call(method, params=None):
            mid["n"] += 1
            cur = mid["n"]
            fut = asyncio.get_event_loop().create_future()
            pending[cur] = fut
            await cdp.send(json.dumps({"id": cur, "method": method, "params": params or {}}))
            return await fut

        async def recv_loop():
            async for raw in cdp:
                msg = json.loads(raw)
                if "id" in msg and msg["id"] in pending:
                    pending[msg["id"]].set_result(msg)

        asyncio.create_task(recv_loop())

        def ok(resp):
            if "error" in resp:
                raise RuntimeError("CDP error: %s" % resp["error"])
            return resp.get("result", {})

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return None
            return r.get("result", {}).get("value")

        async def wait_for(expr, timeout=10):
            end = asyncio.get_event_loop().time() + timeout
            while asyncio.get_event_loop().time() < end:
                v = await js(expr)
                if v:
                    return v
                await asyncio.sleep(0.15)
            return None

        async def wrap(expr):
            return ok(await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB + "\n" + expr}))

        async def shot(name):
            data = ok(await call("Page.captureScreenshot", {"format": "png"}))["data"]
            (OUT / name).write_bytes(__import__("base64").b64decode(data))
            print(f"  shot {OUT / name}")

        await wrap(WRAP)
        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await wait_for("(window.__STRATA_CALLS||[]).some(c=>c.cmd==='strata_items')", 8)

        # ---- 8) Liste startet VOLL, Alle-Pill aktiv ----
        rows0 = await js("craftRows()")
        check("8a Liste startet VOLL (LEER = all)", (rows0 or []) and len(rows0) == 15, str(len(rows0 or [])))
        pills = await js("craftPills()")
        check("1a Alle bleibt PILL/BUTTON (cchip), Einziges", pills == ["All"], str(pills))
        check("1b Alle-Pill Default-aktiv (is-on)",
              await js("document.querySelector('.crafting .cchip')?.classList.contains('is-on')") is True)
        check("2a KEIN natives <select> im Crafting-Modus",
              await js("document.querySelectorAll('.crafting select').length") == 0)
        check("2b Ober-Dropdown (SelectCombo) vorhanden mit Placeholder 'Category'",
              await js("oberInput()?.placeholder") == "Category", str(await js("oberInput()?.placeholder")))
        check("8b Sub-Dropdown bei Alle ausgeblendet (keine Unterkategorie ohne Ober)",
              await js("allScbs().length") == 1, str(await js("allScbs().length")))

        # ---- 2c) Ober-Dropdown öffnen: Maus-Klick auf Eintrag WIRKLICH bedienbar ----
        await js("oberInput().focus(); oberInput().dispatchEvent(new Event('focus')); oberInput().click()")
        await asyncio.sleep(0.3)
        opts = await js("oberDDItems()")
        check("2c Ober-Options: All/FPS/Ship/Mission Items (+Other: null-Item in Daten)",
              opts == ["All", "FPS", "Ship", "Mission Items", "Other"], str(opts))
        await shot("m8-r1b-ober-open.png")
        # Maus-Klick (echtes Click-Event) auf „Ship“ — der alte Fokusduell-
        # Regressionsfall: native GTK-Popups kassierten die Maus; DOM-Popover nicht.
        await js("[...document.querySelectorAll('.crafting .cfchips .scb .scb-item')].find((b)=>b.textContent.trim()==='Ship').click()")
        await asyncio.sleep(0.4)
        check("2d Maus-Klick wählt „Ship“ (Fokusduell-Regression)",
              await js("oberInput()?.value") == "Ship", str(await js("oberInput()?.value")))
        await js("subInput().focus(); subInput().dispatchEvent(new Event('focus')); subInput().click()")
        await asyncio.sleep(0.3)
        sub_opts = await js("subDDItems()")
        check("3a Sub-Options NUR der Oberkategorie (Ship: All + 11)",
              sub_opts is not None and len(sub_opts) == 12 and sub_opts[0] == "All"
              and "Shield" in sub_opts and "Ship Weapon" in sub_opts and "Mining Laser" in sub_opts,
              str(sub_opts))
        await shot("m8-r1b-ship-sub-open.png")

        # ---- 4) Unterkategorie filtert: Shield ----
        await js("subInput().focus(); subInput().dispatchEvent(new Event('focus')); subInput().click()")
        await asyncio.sleep(0.2)
        await js("[...(window.allScbs()[1].querySelectorAll('.scb-item'))].find((b)=>b.textContent.trim()==='Shield').click()")
        await asyncio.sleep(0.4)
        rows = await js("craftRows()")
        check("4a Unter-Filter Shield → nur Shield-Items (2 benannte + 2 Ring-Items)",
              rows and len(rows) == 4
              and "Milspec Shield A" in rows and "Bulwark Shield B" in rows
              and not any("FPS" in n or "Mission" in n for n in rows), str(rows))
        await shot("m8-r1b-shield-filtered.png")

        # ---- 5) Ober-Wechsel resettet Unterkategorie ----
        await js("oberInput().focus(); oberInput().dispatchEvent(new Event('focus')); oberInput().click()")
        await asyncio.sleep(0.2)
        await js("[...document.querySelectorAll('.crafting .cfchips .scb .scb-item')].find((b)=>b.textContent.trim()==='FPS').click()")
        await asyncio.sleep(0.4)
        check("5a Ober-Wechsel FPS resettet Unterkategorie (Feld 'All')",
              await js("subInput()?.value") == "All", str(await js("subInput()?.value")))
        await js("subInput().focus(); subInput().dispatchEvent(new Event('focus')); subInput().click()")
        await asyncio.sleep(0.3)
        sub_opts = await js("subDDItems()")
        check("3b Sub-Options FPS: All + Armor/Weapons/Magazine",
              sub_opts == ["All", "Armor", "Magazine", "Weapons"], str(sub_opts))
        rows = await js("craftRows()")
        check("5b FPS-Liste: 6 FPS-Items (12er-Ring + keine null/Ship)",
              rows and len(rows) == 6, str(len(rows or [])))

        # ---- 6) Mission Items: keine Unterkategorie ----
        await js("oberInput().focus(); oberInput().dispatchEvent(new Event('focus')); oberInput().click()")
        await asyncio.sleep(0.2)
        await js("[...document.querySelectorAll('.crafting .cfchips .scb .scb-item')].find((b)=>b.textContent.trim()==='Mission Items').click()")
        await asyncio.sleep(0.4)
        check("6a Mission Items: Sub-Dropdown ausgeblendet (beides in einem)",
              await js("allScbs().length") == 1, str(await js("allScbs().length")))
        rows = await js("craftRows()")
        check("6b Mission Items: 2 Items", rows and len(rows) == 2, str(rows))

        # ---- 7) Esc schließt Dropdown; Keyboard wählt ----
        await js("oberInput().focus(); oberInput().dispatchEvent(new Event('focus')); oberInput().click()")
        await asyncio.sleep(0.2)
        check("7a Dropdown offen", await js("oberOpen()") is True)
        await js("oberInput().dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))")
        await asyncio.sleep(0.2)
        check("7b Esc schließt Dropdown", await js("oberOpen()") is False)
        # Keyboard: fokussieren → ArrowDown bis „Ship“ → Enter
        await js("oberInput().focus(); oberInput().dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}))")
        await asyncio.sleep(0.1)
        await js("oberInput().dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}))")
        await asyncio.sleep(0.1)
        await js("oberInput().dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}))")
        await asyncio.sleep(0.4)
        check("7c Keyboard (Pfeile+Enter) wählt Oberkategorie",
              await js("oberInput()?.value") == "Ship", str(await js("oberInput()?.value")))

        # ---- 9) null-category → „Sonstige“ nur wenn vorhanden (datengetrieben) ----
        await js("oberInput().focus(); oberInput().dispatchEvent(new Event('focus')); oberInput().click()")
        await asyncio.sleep(0.2)
        opts = await js("oberDDItems()")
        check("9a Sonstige-Option vorhanden (null-category in Daten)",
              opts and "Other" in opts, str(opts))
        await js("[...document.querySelectorAll('.crafting .cfchips .scb .scb-item')].find((b)=>b.textContent.trim()==='Other').click()")
        await asyncio.sleep(0.4)
        rows = await js("craftRows()")
        check("9b Sonstige-Filter → nur Mystery Item", rows == ["Mystery Item"], str(rows))

        # ---- 1c) Alle-Pill = Filter-Reset ----
        await js("document.querySelector('.crafting .cchip').click()")
        await asyncio.sleep(0.4)
        check("1c Alle-Pill resettet Ober+Unter",
              await js("oberInput()?.value") == "All"
              and await js("allScbs().length") == 1, str(await js("oberInput()?.value")))
        rows = await js("craftRows()")
        check("1d Reset → Gesamtliste", rows and len(rows) == 15, str(len(rows or [])))

        # ---- 8c) Kombination mit Hauptsuchfeld ----
        await js("oberInput().focus(); oberInput().dispatchEvent(new Event('focus')); oberInput().click()")
        await asyncio.sleep(0.2)
        await js("[...document.querySelectorAll('.crafting .cfchips .scb .scb-item')].find((b)=>b.textContent.trim()==='Ship').click()")
        await asyncio.sleep(0.3)
        await js("setSearch('milspec')")
        await asyncio.sleep(0.4)
        rows = await js("craftRows()")
        check("8c Ober + Suchfeld kombinierbar", rows == ["Milspec Shield A"], str(rows))
        await js("setSearch('')")
        await shot("m8-r1b-final.png")

    proc.kill()
    try:
        server.kill()
    except ProcessLookupError:
        pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
