#!/usr/bin/env python3
"""CDP-Probe für t_cf1224c5 (M7-F3: Karten-Redesign + Sammel-Pin + i18n/UX).

Belegt auf der echten Svelte-Oberfläche (Tauri-IPC gestubbt, Fixture-Daten):
  1. Karten-Redesign: KEIN Dial-SVG mehr — Kürzel-Block (1.25:1) mit Kürzel +
     Scan-Signatur, Rang-Balken (kontinuierlich) mit Anzeige „#1“ (EN) bzw.
     „Nr. 1“ (DE), Konzentrations-Balken mit 5 Gitterlinien (6 feste Stufen
     bei 1/4/8/15/25 %), kein „N spawn sites“-Zähler.
  2. Responsives Grid: 4 Spalten bei ~3440px-Breite, 2 Spalten schmal,
     Screenshots beider Stände.
  3. Sammel-Signatur-Pin: pin_add mit content.type 'scan-signatures' +
     entries (dedupe nach oreId), zweiter Klick auf dasselbe Erz → pin_update
     mit reduzierter Liste, Toggle auf leer → pin_remove. Kein Strata-
     Vokabular im Payload.
  4. Haupt-Suchfeld filtert die Erz-Liste (keine internen Textfelder mehr).
  5. i18n: DE-Gruppen-Labels („Weltraum (Felder/Gürtel/Lagrange)“), DE-Rang
     „Nr. 1“, Seltenheit „Selten“, Tier-Band-Label „Major 15%+“.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/m7-f3-redesign")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

WRAP = r"""
(function () {
  const ORES = {
    ores: [
      { id: 'beryl_raw', name: 'Beryl (Raw)', slug: 'beryl', category: 'ship', tier: 'rare', scanSignature: 3540, instability: 0.4, resistance: 0.3 },
      { id: 'gold_ore', name: 'Gold (Ore)', slug: 'gold', category: 'ship', tier: 'legendary', scanSignature: 3885, instability: 0.5, resistance: 0.6 },
      { id: 'quartz_raw', name: 'Quartz (Raw)', slug: 'quartz', category: 'fps', tier: 'common', scanSignature: 900, instability: 0.1, resistance: 0.2 }
    ],
    count: 3,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  const LOCS = {
    locations: [
      { id: 'aarON', name: 'Aaron Halo', slug: 'aaron-halo', system: 'Stanton', type: 'lagrange_point' },
      { id: 'cellin', name: 'Cellin', slug: 'cellin', system: 'Stanton', type: 'moon' }
    ],
    count: 2,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  const row = (id, name, system, rankPct, rank, n, conc, tier, tierName) => ({
    locationId: id, location: name, system, rankPct, rank, n, concentration: conc, tier, tierName
  });
  const ORE_LOCS = (sort) => ({
    ore: { id: 'gold_ore', name: 'Gold (Ore)' },
    sort,
    tierLegend: { '1': 'Negligible', '2': 'Trace', '3': 'Minor', '4': 'Notable', '5': 'Major', '6': 'Pure' },
    results: {
      // space mit 4 Orten — belegt die 4-Spalten-Reihe VISUELL im Screenshot
      space: [
        row('grin_a', 'Grin Mining Belt Alpha', 'Stanton', 100, 1, 12, 0.31, 5, 'Major'),
        row('grin_b', 'Grin Mining Belt Beta', 'Stanton', 91, 2, 12, 0.24, 5, 'Major'),
        row('grin_c', 'Grin Mining Belt Gamma', 'Stanton', 78, 3, 12, 0.17, 5, 'Major'),
        row('grin_d', 'Grin Mining Belt Delta', 'Stanton', 63, 4, 12, 0.09, 4, 'Notable')
      ],
      surface: [row('cellin', 'Cellin', 'Stanton', 82, 2, 7, 0.09, 4, 'Notable')],
      cave: [row('jumptown', 'Jumptown', 'Stanton', 54, 3, 4, 0.02, 2, 'Trace')],
      exploration: [row('aarON', 'Aaron Halo', 'Stanton', 40, 4, 2, 0.004, 1, 'Negligible')]
    },
    attribution: 'Data from celd.space / Strata Mining Tools'
  });
  const LOCATION_ORES = {
    location: { id: 'aarON', name: 'Aaron Halo', system: 'Stanton', type: 'lagrange_point' },
    ores: [
      { oreId: 'gold_ore', name: 'Gold (Ore)', concentration: 0.4231, tier: 6, tierName: 'Pure', scanSignature: 3885 },
      { oreId: 'beryl_raw', name: 'Beryl (Raw)', concentration: 0.0812, tier: 4, tierName: 'Notable', scanSignature: 3540 }
    ],
    count: 2,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  window.__STRATA_CALLS = [];
  // Sammel-Pin-Spiegel MIT content (der Repo-Stub speichert content nicht —
  // dieser Wrapper registriert sich NACH dem Repo-Stub und gewinnt).
  window.__PINS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_ores') return ORES;
    if (cmd === 'strata_locations') return LOCS;
    if (cmd === 'strata_ore_locations') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      return ORE_LOCS(a.sort === 'concentration' ? 'concentration' : 'rank');
    }
    if (cmd === 'strata_location_ores') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      return LOCATION_ORES;
    }
    if (cmd === 'pins_state') {
      return { enabled: true, game_running: true, overlay_visible: true,
               pins: window.__PINS.map((p) => ({ ...p, rows: (p.rows || []).map((r) => ({ ...r })) })),
               spotlight_screen: 'DP-3', default_opacity: 100, presets: [40, 60, 100] };
    }
    if (cmd === 'pin_add') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      if (window.__PINS.some((p) => p.pinned && p.view_id === a.view_id && p.screen === 'DP-3' && p.box_index === a.box_index)) return false;
      if (a.key && window.__PINS.some((p) => p.pinned && p.view_id === a.view_id && p.key === a.key)) return false;
      window.__PINS.push({ view_id: a.view_id, screen: 'DP-3', box_index: a.box_index,
        opacity: 100, pinned: true, x: 10, y: 10, scale: 100,
        title: a.title, kpi: a.kpi ?? null, rows: a.rows || [], content: a.content ?? null, key: a.key ?? null });
      return true;
    }
    if (cmd === 'pin_update') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      let hit = false;
      for (const p of window.__PINS) {
        if (p.pinned && p.view_id === a.view_id && p.box_index === a.box_index) {
          p.title = a.title; p.kpi = a.kpi ?? null; p.rows = a.rows || [];
          if (a.content !== undefined) p.content = a.content;
          if (a.key != null) p.key = a.key;
          hit = true;
        }
      }
      return hit;
    }
    if (cmd === 'pin_remove') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      const before = window.__PINS.length;
      window.__PINS = window.__PINS.filter((p) => !(p.view_id === a.view_id && p.screen === a.screen && p.box_index === a.box_index));
      return window.__PINS.length !== before;
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
        "--remote-debugging-port=9238", "--user-data-dir=/tmp/cdp-m7f3",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9238/json/list", timeout=1) as r:
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

        async def set_vp(w, h):
            await call("Emulation.setDeviceMetricsOverride", {"width": w, "height": h, "deviceScaleFactor": 2, "mobile": False})
            await asyncio.sleep(0.5)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        VK = {"F5": 116, "Escape": 27, "Enter": 13}
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

        await call("Page.enable")
        await call("Runtime.enable")
        await set_vp(1920, 1080)
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": WRAP})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)

        # ---- 1) Mining öffnen, Erz-Detail → Karten-Redesign ----
        await key("F5")
        open_m = await js("!!document.querySelector('.mining')")
        check("f5-opens-mining", bool(open_m))
        await js("[...document.querySelectorAll('.mining .mrow-main')][1].click()")  # Gold
        await asyncio.sleep(0.8)
        cards = int(await js("document.querySelectorAll('.mining .dcard').length"))
        check("detail-cards-7", cards == 7, "cards=" + str(cards))
        dial = await js("document.querySelectorAll('.mining .dial').length")
        check("no-dial-svg", dial == 0, "dials=" + str(dial))
        abbr = (await js("(document.querySelector('.mining .abbr')||{}).textContent") or "").strip()
        check("abbr-block-big-kuerzel", abbr == "Go", abbr)  # Gold (Ore) → Go, nicht GO
        sig = (await js("(document.querySelector('.mining .abbr-sig')||{}).textContent") or "").strip()
        check("abbr-block-signature", sig == "3885", sig)
        box = await js("""JSON.stringify((function(){
            var b=document.querySelector('.mining .abbr-block');
            var r=b?b.getBoundingClientRect():null;
            return r?{w:Math.round(r.width),h:Math.round(r.height)}:null;})())""")
        if box and box != "null":
            b = json.loads(box)
            ratio = b["h"] / max(1, b["w"])
            check("abbr-block-1.25-ratio", 1.1 <= ratio <= 1.4, json.dumps(b))
        else:
            check("abbr-block-1.25-ratio", False, box)
        # Rang als Platzierung: „#1“ (EN default), kein „100 %“-Rangtext
        rank_text = (await js("[...document.querySelectorAll('.mining .dnum-v')].map(function(e){return e.textContent.trim();}).join('|')") or "")
        check("rank-as-place", all(v in rank_text for v in ["#1","#2","#3","#4"]), rank_text[:160])
        check("no-rank-percent", "100 %" not in rank_text.split("|")[0], rank_text[:80])
        # Konzentrations-Balken: 5 Gitterlinien je Karte (6 feste Stufen)
        segs = int(await js("document.querySelectorAll('.mining .seg-line').length"))
        check("conc-seg-lines-35", segs == 7 * 5, "segs=" + str(segs))
        # Punkt 4: kein „N spawn sites“
        sub = (await js("[...document.querySelectorAll('.mining .dcard-sub')].map(function(e){return e.textContent.trim();}).join('|')") or "")
        check("no-spawn-counter", "spawn" not in sub.lower() and "12" not in sub, sub[:120])
        await shot("01-cards-1920.png")

        # ---- 2) Responsives Grid: 3440px → 4 Spalten, schmal → 2/1 ----
        await set_vp(3440, 1200)
        cols_wide = int(await js("getComputedStyle(document.querySelector('.mining .dcards')).gridTemplateColumns.split(' ').length"))
        check("grid-4cols-3440", cols_wide == 4, "cols=" + str(cols_wide))
        await shot("02-cards-4col-3440.png")
        await set_vp(1800, 1000)
        cols_mid = int(await js("getComputedStyle(document.querySelector('.mining .dcards')).gridTemplateColumns.split(' ').length"))
        check("grid-2cols-1800", cols_mid == 2, "cols=" + str(cols_mid))
        await shot("03-cards-2col-1800.png")
        await set_vp(900, 900)
        cols_narrow = int(await js("getComputedStyle(document.querySelector('.mining .dcards')).gridTemplateColumns.split(' ').length"))
        check("grid-narrow-shrinks", cols_narrow < cols_mid, "cols=" + str(cols_narrow))
        await set_vp(1920, 1080)

        # ---- 3) Sammel-Signatur-Pin: add → update → remove ----
        await key("Escape")  # Detail zu
        await asyncio.sleep(0.4)
        await js("window.__STRATA_CALLS.length = 0; window.__PINS.length = 0;")
        await js("[...document.querySelectorAll('.mining .mpin')][0].click()")  # Beryl pinnen
        await asyncio.sleep(0.6)
        add = json.loads(await js("JSON.stringify(window.__STRATA_CALLS.find(function(c){return c.cmd==='pin_add'}) || null)"))
        content = ((add or {}).get("args", {}).get("content") or {})
        entries = content.get("entries", [])
        check("collect-pin-add-type", content.get("type") == "scan-signatures", json.dumps(content)[:200])
        check("collect-pin-entry-generic", bool(entries) and set(entries[0].keys()) == {"oreId", "label", "value"}, json.dumps(entries)[:200])
        check("collect-pin-key", (add or {}).get("args", {}).get("key") == "scan-signatures", json.dumps((add or {}).get("args", {}).get("key")))
        rows = ((add or {}).get("args", {}).get("rows") or [])
        check("collect-pin-row-idx-strings", [r.get("idx") for r in rows] == ["0"], json.dumps(rows))
        # zweites Erz → pin_update mit 2 Einträgen
        await js("window.__STRATA_CALLS.length = 0;")
        await js("[...document.querySelectorAll('.mining .mpin')][1].click()")  # Gold
        await asyncio.sleep(0.6)
        upd = json.loads(await js("JSON.stringify(window.__STRATA_CALLS.find(function(c){return c.cmd==='pin_update'}) || null)"))
        uentries = (((upd or {}).get("args", {}).get("content") or {}).get("entries") or [])
        check("collect-pin-update-2-entries", len(uentries) == 2, json.dumps(uentries)[:200])
        # Pinned-Marker sichtbar (2 is-pinned-Buttons)
        pinned = int(await js("document.querySelectorAll('.mining .mpin.is-pinned').length"))
        check("pinned-markers-2", pinned == 2, "pinned=" + str(pinned))
        # Toggle Gold ab → pin_update mit 1 Eintrag; Beryl ab → pin_remove
        await js("window.__STRATA_CALLS.length = 0;")
        await js("[...document.querySelectorAll('.mining .mpin')][1].click()")
        await asyncio.sleep(0.6)
        upd2 = json.loads(await js("JSON.stringify(window.__STRATA_CALLS.find(function(c){return c.cmd==='pin_update'}) || null)"))
        u2 = (((upd2 or {}).get("args", {}).get("content") or {}).get("entries") or [])
        check("collect-pin-toggle-off-1-entry", len(u2) == 1 and u2[0]["oreId"] == "beryl_raw", json.dumps(u2)[:160])
        await js("window.__STRATA_CALLS.length = 0;")
        await js("[...document.querySelectorAll('.mining .mpin')][0].click()")
        await asyncio.sleep(0.6)
        rem = json.loads(await js("JSON.stringify(window.__STRATA_CALLS.find(function(c){return c.cmd==='pin_remove'}) || null)"))
        check("collect-pin-empty-removed", rem is not None, json.dumps(rem))
        await key("Escape")

        # ---- 4) Haupt-Suchfeld filtert die Erz-Liste ----
        await key("F5")
        await asyncio.sleep(0.4)
        all_rows = int(await js("document.querySelectorAll('.mining .mlist .mrow').length"))
        await js("""(function(){var q=document.querySelector('.q');q.value='gold';q.dispatchEvent(new Event('input',{bubbles:true}));})()""")
        await asyncio.sleep(0.6)
        filt_rows = int(await js("document.querySelectorAll('.mining .mlist .mrow').length"))
        check("main-search-filters-ores", all_rows == 3 and filt_rows == 1, f"all={all_rows} filt={filt_rows}")
        await js("""(function(){var q=document.querySelector('.q');q.value='';q.dispatchEvent(new Event('input',{bubbles:true}));})()""")
        await asyncio.sleep(0.4)
        # keine internen Textfelder mehr
        inner = int(await js("document.querySelectorAll('.mining input').length"))
        check("no-inner-text-fields", inner == 0, "inputs=" + str(inner))

        # ---- 5) i18n DE: Reload mit __probeLang=de (M3-Muster des Repo-Stubs —
        # der Live-Switch läuft über setLanguagePersisted in der Settings-UI,
        # der Probe-Reload setzt die Sprache deterministisch) ----
        await js("try{localStorage.setItem('__probeLang','de')}catch(e){}")
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)
        await key("F5")
        await asyncio.sleep(0.4)
        rarity_chip = (await js("[...document.querySelectorAll('.mining .mchip.mtier')].map(function(b){return b.textContent.trim();}).join('|')") or "")
        check("de-rarity-labels", "Selten" in rarity_chip and "Legendär" in rarity_chip, rarity_chip)
        await js("[...document.querySelectorAll('.mining .mrow-main')][1].click()")
        await asyncio.sleep(0.8)
        group = (await js("(document.querySelector('.mining .mg-title')||{}).textContent") or "").strip()
        check("de-group-space-label", group == "Weltraum (Felder/Gürtel/Lagrange)", group)
        rank_de = (await js("[...document.querySelectorAll('.mining .dnum-v')].map(function(e){return e.textContent.trim();}).join('|')") or "")
        check("de-rank-nr", "Nr. 1" in rank_de and "#1" not in rank_de, rank_de[:120])
        badge = (await js("(document.querySelector('.mining .dcard .mtier-badge')||{}).textContent") or "").strip()
        check("de-tier-band-badge", badge.startswith("Major 15%+"), badge)
        await shot("04-cards-de.png")
        # ---- 6) EN: Gruppen-Label + Ort-Detail mit übersetztem Typ ----
        await js("try{localStorage.setItem('__probeLang','en')}catch(e){}")
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)
        await key("F5")
        await asyncio.sleep(0.4)
        await js("[...document.querySelectorAll('.mining .mrow-main')][1].click()")
        await asyncio.sleep(0.8)
        group_en = (await js("(document.querySelector('.mining .mg-title')||{}).textContent") or "").strip()
        check("en-group-space-label", group_en == "Space (fields/belts/Lagrange)", group_en)
        await key("Escape")
        await asyncio.sleep(0.4)
        await js("[...document.querySelectorAll('.mining .mtab')].find(function(b){return b.textContent.trim().toLowerCase().includes('location')}).click()")
        await asyncio.sleep(0.4)
        type_chip = (await js("[...document.querySelectorAll('.mining .mfchips .mchip')].map(function(b){return b.textContent.trim();}).join('|')") or "")
        check("type-chips-translated", "Lagrange Point" in type_chip and "Moon" in type_chip, type_chip)
        await js("document.querySelector('.mining .mlist .lrow .mrow-main').click()")
        await asyncio.sleep(0.8)
        sub2 = (await js("(document.querySelector('.mining .md-sub')||{}).textContent") or "").strip()
        check("loc-detail-type-translated", sub2 == "Stanton · Lagrange Point", sub2)
        await shot("05-location-detail-en.png")

        try:
            server.terminate(); proc.terminate()
        except ProcessLookupError:
            pass  # chromium beendet sich teils selbst — Cleanup ist best effort
        print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

asyncio.run(main())
