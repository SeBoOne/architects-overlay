#!/usr/bin/env python3
"""CDP-Probe für t_bb3303d0 (M7-K2: Mining-Mode-UI, F5).

Belegt auf der echten Svelte-Oberfläche (echte Logik, Tauri-IPC gestubbt):
  1. Rail: 6 Einträge inkl. Mining (F5) — F5-Taste öffnet den Modus
     (außerhalb der Resync-Kontexte Market/Detail).
  2. Key-Missing-State: ohne celd_-Key → Erklär-Card mit api-keys-Link.
  3. Mit Key: Erz-Liste alphabetisch, Typeahead filtert (LEER = alles),
     Filter category/tier; Pin-Button feuert pin_add mit Content-Typ
     `scan-signature` + stabilen Key.
  4. Detail „Orte pro Erz": Gruppen space/exploration/cave/surface mit
     Gruppentitel + Nicht-Vergleichbarkeits-Hinweis, Two-Ring-Dials,
     Sort-Umschalter (rank ↔ concentration, neuer IPC-Call), 6-Stufen-
     Legende, Attribution-Footer.
  5. Esc-Kaskade: Detail → Liste → Modus verlassen (kein overlay-hide).

Strata-Commands werden über einen Invoke-Wrapper gestubbt (Fixture-Daten),
der sich NACH dem Repo-Stub registriert; `settings_get*`-Responses werden
mit `strata_enabled: true` angereichert (Probe-A-Flag `__stubState.strata`).
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/m7-k2-mining")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

# ---- Strata-Wrapper: registriert sich NACH dem Repo-Stub --------------------
WRAP = r"""
(function () {
  const ORES = {
    ores: [
      { id: 'beryl_raw', name: 'Beryl', slug: 'beryl', category: 'ship', tier: 2, scan_signature: 12, instability: 0.4, resistance: 0.3 },
      { id: 'copper_raw', name: 'Copper', slug: 'copper', category: 'ship', tier: 1, scan_signature: 0, instability: 0.1, resistance: 0.2 },
      { id: 'gold_raw', name: 'Gold', slug: 'gold', category: 'fps', tier: 4, scan_signature: 30, instability: 0.5, resistance: 0.6 },
      { id: 'quantainium_raw', name: 'Quantainium', slug: 'quantainium', category: 'ship', tier: 6, scan_signature: 90, instability: 0.9, resistance: 0.9 },
      { id: 'titanium_raw', name: 'Titanium', slug: 'titanium', category: 'ship', tier: 3, scan_signature: null, instability: 0.2, resistance: 0.4 }
    ],
    count: 5,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  const LOCS = {
    locations: [
      { id: 'aarON', name: 'Aaron Halo', slug: 'aaron-halo', system: 'Stanton', kind: 'exploration_region' },
      { id: 'cellin', name: 'Cellin', slug: 'cellin', system: 'Stanton', kind: 'moon' },
      { id: 'grin_a', name: 'Grin Mining Belt Alpha', slug: 'grin-mining-belt-alpha', system: 'Stanton', kind: 'belt' },
      { id: 'jumptown', name: 'Jumptown', slug: 'jumptown', system: 'Stanton', kind: 'dungeon' }
    ],
    count: 4,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  const row = (id, name, system, rankPct, rank, n, conc, tier, tierName) => ({
    locationId: id, location: name, system, rankPct, rank, n, concentration: conc, tier, tierName
  });
  const ORE_LOCS = (sort) => ({
    ore: { id: 'gold_raw', name: 'Gold' },
    sort,
    tierLegend: { '0': 'None', '1': 'Barely Any', '2': 'A Trace', '3': 'Some', '4': 'A Decent Amount', '5': 'Lots', '6': 'Almost Pure' },
    results: {
      space: [row('grin_a', 'Grin Mining Belt Alpha', 'Stanton', 100, 1, 12, 0.31, 5, 'Lots')],
      surface: [row('cellin', 'Cellin', 'Stanton', 82, 2, 7, 0.09, 4, 'A Decent Amount')],
      cave: [row('jumptown', 'Jumptown', 'Stanton', 54, 3, 4, 0.02, 2, 'A Trace')],
      exploration: [row('aarON', 'Aaron Halo', 'Stanton', 40, 4, 2, 0.004, 1, 'Barely Any')]
    },
    attribution: 'Data from celd.space / Strata Mining Tools'
  });
  window.__STRATA_FIXTURE = { ORES, LOCS, ORE_LOCS };
  window.__STRATA_CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') {
      return { enabled: true, key_set: (function(){try{return localStorage.getItem('__probeStrataKey')==='1'}catch(e){return false}})(), key_ref: 'default', tier_default: 4 };
    }
    if (cmd === 'strata_ores') return ORES;
    if (cmd === 'strata_locations') return LOCS;
    if (cmd === 'strata_ore_locations') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      return ORE_LOCS(a.sort === 'concentration' ? 'concentration' : 'rank');
    }
    if (cmd === 'pin_add') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      return true;
    }
    if (cmd === 'strata_set_key') {
      try { localStorage.setItem('__probeStrataKey', a.key != null && String(a.key).trim() !== '' ? '1' : '0'); } catch (e) {}
      return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    }
    const r = await orig(cmd, args);
    if (cmd === 'settings_get' || cmd.startsWith('settings_set') || cmd === 'settings_reset_mode_hotkeys') {
      if (r && typeof r === 'object') r.strata_enabled = true;
    }
    return r;
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
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9236", "--user-data-dir=/tmp/cdp-m7k2",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9236/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        server.terminate(); proc.terminate()
        raise RuntimeError("chromium CDP did not come up")

    async with websockets.connect(ws, max_size=None) as cdp:
        msg_id = 0
        pending = {}
        async def call(method, params=None):
            nonlocal msg_id
            msg_id += 1
            mid = msg_id
            fut = asyncio.get_event_loop().create_future()
            pending[mid] = fut
            await cdp.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
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

        await call("Page.enable")
        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": WRAP})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        VK = {"F1": 112, "F2": 113, "F5": 116, "F12": 123, "Escape": 27, "Enter": 13}
        async def key(k):
            vk = VK[k]
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.4)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", OUT / name)

        def set_input_expr(sel, text):
            return ("(function(){var i=document.querySelector(" + json.dumps(sel) + ");"
                    "if(!i) return 'NO-ELEMENT';"
                    "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                    "set.call(i,'');"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                    "set.call(i," + json.dumps(text) + ");"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                    "return i.value;})()")

        # Probe-Isolation: Flag vom Vorlauf löschen (Persistenz im Profil).
        await js("try{localStorage.removeItem('__probeStrataKey')}catch(e){}; location.reload();")
        await asyncio.sleep(2.5)

        # ---- 1) Rail: 6 Einträge inkl. Mining (F5) ----
        keys = json.loads(await js("JSON.stringify([...document.querySelectorAll('.rentry .rkey')].map(function(e){return e.textContent.trim();}))"))
        check("rail-has-f5-mining", "F5" in keys and len(keys) == 7, str(keys))
        lbl = await js("[...document.querySelectorAll('.rentry')].find(function(e){return e.textContent.includes('Mining')}) ? 'yes' : 'no'")
        check("rail-mining-entry", lbl == "yes")

        # ---- 2) Key-Missing-State (Stub: key_set=false) ----
        await key("F5")
        card = await js("!!document.querySelector('.mining .mcard')")
        check("f5-opens-mining", bool(card))
        kt = (await js("document.querySelector('.mining .mc-h') ? document.querySelector('.mining .mc-h').textContent : 'NONE'")).strip()
        check("key-missing-card", "Strata API key" in kt, kt)
        link = await js("var a=document.querySelector('.mining .mc-link'); a ? a.href : 'NONE'")
        check("key-missing-link", "strata.celd.space/api-keys" in (link or ""), link)
        attr = (await js("document.querySelector('.mining .mattr') ? document.querySelector('.mining .mattr').textContent : 'NONE'")).strip()
        check("attribution-footer-on-missing", "celd.space" in attr, attr)
        await shot("01-mining-key-missing.png")

        # ---- 3) Mit Key: Erz-Suche ----
        await js("try{localStorage.setItem('__probeStrataKey','1')}catch(e){}; location.reload();")
        await asyncio.sleep(2.5)
        await key("F5")
        rows = int(await js("document.querySelectorAll('.mining .mlist .mrow').length"))
        check("ore-list-all-5", rows == 5, "rows=" + str(rows))
        names = json.loads(await js("JSON.stringify([...document.querySelectorAll('.mining .mrow-name')].map(function(e){return e.textContent.trim();}))"))
        check("ore-list-alphabetical", names == sorted(names), str(names))
        # Typeahead: leer = alles, 'gold' filtert
        await js(set_input_expr(".mining .mq", "GOLD"))
        await asyncio.sleep(0.3)
        rows = int(await js("document.querySelectorAll('.mining .mlist .mrow').length"))
        check("ore-typeahead-filters", rows == 1, "rows=" + str(rows))
        await js(set_input_expr(".mining .mq", ""))
        await asyncio.sleep(0.3)
        # category-Filter fps → nur Gold
        await js("[...document.querySelectorAll('.mining .mchip')].find(function(b){return b.textContent.trim()==='FPS'}).click()")
        await asyncio.sleep(0.3)
        rows = int(await js("document.querySelectorAll('.mining .mlist .mrow').length"))
        check("ore-filter-fps", rows == 1, "rows=" + str(rows))
        # tier-Filter 2 zurücksetzen + category alle → 5
        await js("[...document.querySelectorAll('.mining .mchip')].find(function(b){return b.textContent.trim()==='All'}).click()")
        await asyncio.sleep(0.3)
        rows = int(await js("document.querySelectorAll('.mining .mlist .mrow').length"))
        check("ore-filter-reset-all", rows == 5, "rows=" + str(rows))
        await shot("02-mining-ores.png")

        # ---- 4) Pin-Button: pin_add mit scan-signature-Content ----
        await js("window.__STRATA_CALLS.length = 0;")
        await js("[...document.querySelectorAll('.mining .mpin')][3].click()")
        await asyncio.sleep(0.5)
        pin = json.loads(await js("JSON.stringify(window.__STRATA_CALLS[0] || null)"))
        ok_pin = pin and pin["cmd"] == "pin_add" and pin["args"]["content"]["type"] == "scan-signature"
        check("pin-add-scan-signature", bool(ok_pin), json.dumps(pin)[:200] if pin else "no call")
        if ok_pin:
            check("pin-key-stable", pin["args"]["key"] == "scan-signature:quantainium_raw", pin["args"].get("key"))

        # ---- 5) Detail „Orte pro Erz" ----
        await js("[...document.querySelectorAll('.mining .mrow-main')][2].click()")
        await asyncio.sleep(0.8)
        heads = json.loads(await js("JSON.stringify([...document.querySelectorAll('.mining .mg-title')].map(function(e){return e.textContent.trim();}))"))
        check("detail-4-groups", len(heads) == 4, str(heads))
        notes = int(await js("document.querySelectorAll('.mining .mg-note').length"))
        check("group-note-visible", notes == 4, "notes=" + str(notes))
        dials = int(await js("document.querySelectorAll('.mining .dial').length"))
        check("dials-4", dials == 4, "dials=" + str(dials))
        badges = int(await js("document.querySelectorAll('.mining .mtier-badge').length"))
        check("tier-badges-4", badges >= 4, "badges=" + str(badges))
        legend = int(await js("document.querySelectorAll('.mining .ml-item').length"))
        check("legend-6-bands", legend == 6, "legend=" + str(legend))
        tip = (await js("var d=document.querySelectorAll('.mining .dtip')[0]; d ? d.textContent : 'NONE'")).strip()
        check("tooltip-exact-values", "Rank" in tip and "Concentration" in tip, tip)
        attr = (await js("document.querySelector('.mining .mattr').textContent")).strip()
        check("attribution-footer-detail", "celd.space" in attr, attr)
        await shot("03-mining-detail.png")
        # Sort-Umschalter → neuer IPC-Call mit sort=concentration
        await js("window.__STRATA_CALLS.length = 0;")
        await js("[...document.querySelectorAll('.mining .msort .mchip')].find(function(b){return b.textContent.trim()!=='Rank'}).click()")
        await asyncio.sleep(0.6)
        calls = json.loads(await js("JSON.stringify(window.__STRATA_CALLS.map(function(c){return {cmd:c.cmd, sort:c.args.sort};}))"))
        check("sort-switch-ipc", any(c["cmd"] == "strata_ore_locations" and c.get("sort") == "concentration" for c in calls), json.dumps(calls))

        # ---- 6) Esc-Kaskade: Detail → Liste → Modus raus ----
        await key("Escape")
        d = await js("!document.querySelector('.mining .md-head') && !!document.querySelector('.mining')")
        check("esc-detail-back-to-list", bool(d))
        await key("Escape")
        gone = await js("!document.querySelector('.mining')")
        check("esc-exits-mode", bool(gone))
        hide = await js("JSON.stringify((window.__invokes||[]).filter(function(c){return c.cmd==='overlay_set_visible'}).length)")
        check("esc-no-overlay-hide", hide == "0", hide)

        # ---- 7) F5-Koexistenz: im Market-Modus bleibt F5 der Resync ----
        await key("F2")
        await asyncio.sleep(0.6)
        await js("window.__invokes = (window.__invokes||[]).filter(function(c){return c.cmd!=='sync_now';});")
        await key("F5")
        await asyncio.sleep(0.6)
        mining_open = await js("!!document.querySelector('.mining')")
        syncs = int(await js("(window.__invokes||[]).filter(function(c){return c.cmd==='get_marketplace'||c.cmd==='sync_now';}).length"))
        check("f5-in-market-still-resync", not mining_open and syncs >= 1, f"mining={mining_open} syncs={syncs}")
        await shot("04-market-f5-resync.png")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

asyncio.run(main())
