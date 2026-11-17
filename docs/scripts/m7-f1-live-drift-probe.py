#!/usr/bin/env python3
"""CDP-Probe für t_8292a1a3 (M7-F1: Live-Contract-Drift, F5/STRG+F5, Rail).

Belegt auf der echten Svelte-Oberfläche (echte Logik, Tauri-IPC gestubbt) —
die ALTE Probe (m7-k2-mining-probe.py) bleibt als historischer Beleg stehen,
dieser Stand ändert die Erwartungen:

  1. Bug 3 — Rail-Sichtbarkeit: OHNE Key (strata_status.key_set=false) hat
     die Rail KEINEN Mining-Eintrag (kein grauer Slot) und F5 tut NICHTS
     (weder Modus noch Resync). MIT Key: 7 Einträge inkl. Mining, F5 öffnet.
  2. Bug 1 — Rarity: Erz-Liste rendert Rarity-STRINGS als Badge
     (common/uncommon/... statt Zahlen), Rarity-Filter-Chips ersetzen den
     alten Tier-Filter 1–6; Detail-Dial bleibt unverändert (Tier 1–6 +
     Legende). Live-Contract: instability 350! / resistance -0.4 kommen
     unclamp't durch die UI (Anzeige rendert sie roh).
  3. Bug 2 — F5-Trennung: F5 ALLEIN triggert NUR den Mining-Modus-Wechsel
     (auch im Market-Modus — NIE Resync); STRG+F5 löst den Resync aus
     (sync-Pfad), Hint-Zeilen zeigen STRG+F5.

Strata-Commands werden über einen Invoke-Wrapper gestubbt (Fixture-Daten
im Live-Contract-Format), `settings_get*`-Responses mit `strata_enabled:
true` angereichert; `key_set` kommt aus dem Probe-LocalStorage-Flag.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/m7-f1-live-drift")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

# ---- Strata-Wrapper: registriert sich NACH dem Repo-Stub --------------------
WRAP = r"""
(function () {
  // Live-Contract (M7-F1): tier = String-Rarity, instability/resistance
  // unbeschränkte f64 (350! / negativ), locations system null-fähig.
  const ORES = {
    ores: [
      { id: 'aluminium_ore', name: 'Aluminum', slug: 'aluminum', category: 'ship', tier: 'common', scan_signature: 4285, instability: 0, resistance: -0.4 },
      { id: 'beryl_raw', name: 'Beryl', slug: 'beryl', category: 'ship', tier: 'uncommon', scan_signature: 12, instability: 0.4, resistance: 0.3 },
      { id: 'copper_raw', name: 'Copper', slug: 'copper', category: 'ship', tier: 'common', scan_signature: 0, instability: 0.1, resistance: 0.2 },
      { id: 'gold_raw', name: 'Gold', slug: 'gold', category: 'fps', tier: 'rare', scan_signature: 30, instability: 350, resistance: 0.6 },
      { id: 'quantainium_raw', name: 'Quantainium', slug: 'quantainium', category: 'ship', tier: 'legendary', scan_signature: 90, instability: 0.9, resistance: 0.9 },
      { id: 'titanium_raw', name: 'Titanium', slug: 'titanium', category: 'ship', tier: 'epic', scan_signature: null, instability: 0.2, resistance: 0.4 }
    ],
    count: 6,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  const LOCS = {
    locations: [
      { id: 'aarON', name: 'Aaron Halo', slug: 'aaron-halo', system: 'Stanton', type: 'asteroid_field' },
      { id: 'cellin', name: 'Cellin', slug: 'cellin', system: 'Stanton', type: 'moon' },
      { id: 'grin_a', name: 'Grin Mining Belt Alpha', slug: 'grin-mining-belt-alpha', system: 'Stanton', type: 'belt' },
      { id: 'jumptown', name: 'Jumptown', slug: 'jumptown', system: null, type: 'dungeon' }
    ],
    count: 4,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  // Feldnamen snake_case — die Komponente liest die gemappten DTO-Felder
  // (rank_pct/tier_name/location_id), der Stub ersetzt die Tauri-Schicht.
  const row = (id, name, system, rankPct, rank, n, conc, tier, tierName) => ({
    location_id: id, location: name, system, rank_pct: rankPct, rank, n, concentration: conc, tier, tier_name: tierName
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
  const keySet = function(){ try { return localStorage.getItem('__probeStrataKey') === '1'; } catch (e) { return false; } };
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') {
      return { enabled: true, key_set: keySet(), key_ref: 'default', tier_default: 4 };
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
        "--remote-debugging-port=9237", "--user-data-dir=/tmp/cdp-m7f1",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9237/json/list", timeout=1) as r:
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
        async def key(k, ctrl=False):
            vk = VK[k]
            mods = 2 if ctrl else 0  # Input.Modifier: Ctrl = 2
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
                                                  "modifiers": mods})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
                                                  "modifiers": mods})
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

        # ---- 1) Bug 3: disabled — Rail OHNE Mining-Eintrag, F5 wirkungslos ----
        keys = json.loads(await js("JSON.stringify([...document.querySelectorAll('.rentry .rkey')].map(function(e){return e.textContent.trim();}))"))
        check("rail-disabled-no-f5", "F5" not in keys and len(keys) == 6, str(keys))
        has_mining = await js("[...document.querySelectorAll('.rentry')].some(function(e){return e.textContent.includes('Mining')})")
        check("rail-disabled-no-mining-entry", has_mining is False, str(has_mining))
        await key("F5")
        mining_open = await js("!!document.querySelector('.mining')")
        syncs = int(await js("(window.__invokes||[]).filter(function(c){return c.cmd==='get_marketplace'||c.cmd==='sync_now';}).length"))
        check("f5-disabled-noop", mining_open is False and syncs == 0, f"mining={mining_open} syncs={syncs}")
        await shot("01-rail-disabled-no-mining.png")

        # ---- 2) Mit Key: Rail mit Mining, F5 öffnet ----
        await js("try{localStorage.setItem('__probeStrataKey','1')}catch(e){}; location.reload();")
        await asyncio.sleep(2.5)
        keys = json.loads(await js("JSON.stringify([...document.querySelectorAll('.rentry .rkey')].map(function(e){return e.textContent.trim();}))"))
        check("rail-enabled-has-f5", "F5" in keys and len(keys) == 7, str(keys))
        await key("F5")
        check("f5-opens-mining", await js("!!document.querySelector('.mining')") is True)

        # ---- 3) Bug 1: Rarity-Badges + Rarity-Filter ----
        badges = json.loads(await js("JSON.stringify([...document.querySelectorAll('.mining .mrow .mtier-badge')].map(function(e){return e.textContent.trim();}))"))
        check("badges-are-rarity-strings", badges and all(b in ("common","uncommon","rare","epic","legendary") for b in badges), str(badges))
        chips = json.loads(await js("JSON.stringify([...document.querySelectorAll('.mining .mfchips .mchip')].map(function(e){return e.textContent.trim();}))"))
        for rv in ("common", "uncommon", "rare", "epic", "legendary"):
            check("rarity-chip-" + rv, rv in chips, str(chips))
        check("no-numeric-tier-chips", not any(c in ("1","2","3","4","5","6") for c in chips), str(chips))
        # Filter 'rare' → nur Gold
        await js("[...document.querySelectorAll('.mining .mfchips .mchip')].find(function(b){return b.textContent.trim()==='rare'}).click()")
        await asyncio.sleep(0.3)
        rows = int(await js("document.querySelectorAll('.mining .mlist .mrow').length"))
        check("rarity-filter-rare", rows == 1, "rows=" + str(rows))
        await js("[...document.querySelectorAll('.mining .mfchips .mchip')].filter(function(b){return b.textContent.trim()==='All'})[1].click()")
        await asyncio.sleep(0.3)
        rows = int(await js("document.querySelectorAll('.mining .mlist .mrow').length"))
        check("rarity-filter-reset", rows == 6, "rows=" + str(rows))
        await shot("02-mining-ores-rarity.png")

        # ---- 4) Detail unverändert: Tier 1–6-Dial + 6er-Legende ----
        await js("[...document.querySelectorAll('.mining .mrow-main')].find(function(b){return b.textContent.includes('Gold')}).click()")
        await asyncio.sleep(0.8)
        dials = int(await js("document.querySelectorAll('.mining .dial').length"))
        check("detail-dials-4", dials == 4, "dials=" + str(dials))
        legend = int(await js("document.querySelectorAll('.mining .ml-item').length"))
        check("detail-legend-6", legend == 6, "legend=" + str(legend))
        dbadges = json.loads(await js("JSON.stringify([...document.querySelectorAll('.mining .mdetail .mtier-badge')].map(function(e){return e.textContent.trim();}))"))
        check("detail-badges-tier-names", dbadges and all(b in ("Lots","A Decent Amount","A Trace","Barely Any") for b in dbadges), str(dbadges))
        attr = (await js("document.querySelector('.mining .mattr') ? document.querySelector('.mining .mattr').textContent : 'NONE'")).strip()
        check("attribution-footer", "celd.space" in attr, attr)
        await shot("03-mining-detail-unchanged.png")
        await key("Escape")

        # ---- 5) Bug 2: F5 allein = Mining-Wechsel, NIE Resync; STRG+F5 = Resync ----
        await key("F2")
        await asyncio.sleep(1.6)  # Market-Load abwarten, dann Marker resetten
        await js("window.__invokes = (window.__invokes||[]).filter(function(c){return c.cmd!=='sync_now';});")
        await key("F5")
        await asyncio.sleep(0.6)
        mining_open = await js("!!document.querySelector('.mining')")
        syncs = int(await js("(window.__invokes||[]).filter(function(c){return c.cmd==='sync_now';}).length"))
        check("f5-alone-in-market-no-resync", mining_open is True and syncs == 0, f"mining={mining_open} syncs={syncs}")
        # Zurück in den Market, dann STRG+F5 → Resync feuert
        await key("F2")
        await asyncio.sleep(1.6)  # Market-Load abwarten, dann Marker resetten
        # Marker: Delta der Sync-relevanten Calls (get_marketplace/sync_now) —
        # der Market-Einstieg selbst feuert get_marketplace asynchron nach.
        before = int(await js("(window.__invokes||[]).filter(function(c){return c.cmd==='get_marketplace'||c.cmd==='sync_now';}).length"))
        await key("F5", ctrl=True)
        await asyncio.sleep(1.2)
        after = int(await js("(window.__invokes||[]).filter(function(c){return c.cmd==='get_marketplace'||c.cmd==='sync_now';}).length"))
        check("ctrl-f5-resync-fires", after > before, f"before={before} after={after}")
        mining_after = await js("!!document.querySelector('.mining')")
        check("ctrl-f5-no-mode-switch", mining_after is False)
        # Hint-Zeile zeigt STRG+F5 (nicht F5 allein)
        hint = (await js("var h=document.querySelector('.hints'); h ? h.textContent : 'NONE'"))
        check("hint-shows-ctrl-f5", "F5" in hint and "F5" not in hint.replace("Ctrl+F5", "").replace("Strg+F5", ""), hint[:120])
        await shot("04-market-ctrl-f5-resync.png")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

asyncio.run(main())
