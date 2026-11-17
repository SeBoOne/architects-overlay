#!/usr/bin/env python3
"""CDP-Probe für t_6f54ad25 (M7-F2: Ort-Detail + Karten-Ansicht + pin_add idx).

Belegt auf der echten Svelte-Oberfläche (Tauri-IPC gestubbt, Fixture-Daten):
  1. Karten-Ansicht (User-Option 2): Detail-Ergebnisse als .dcard-Grid,
     Two-Ring-Dial ≥ 64px gerendert, rank/concentration als Zahl+Balken-Zeile
     (.dnum/.dbar) OHNE Hover sichtbar.
  2. Ort-Suche: Klick auf eine Ort-Zeile feuert `strata_location_ores`,
     Detail „Erze am Ort" (.lorow-line) mit Name/Konzentration/Tier-Badge/
     Scan-Signatur, Back-Pfeil + ESC zurück.
  3. pin_add Wire-Contract: rows[].idx sind STRINGS ('0'/'1'/'2').

Strata-Commands werden über einen Invoke-Wrapper gestubbt (Fixture-Daten),
der sich NACH dem Repo-Stub registriert; `settings_get*`-Responses werden
mit `strata_enabled: true` angereichert.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/m7-f2-location-detail")))
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
      { id: 'beryl_raw', name: 'Beryl', slug: 'beryl', category: 'ship', tier: 2, scan_signature: 12, instability: 0.4, resistance: 0.3 },
      { id: 'gold_raw', name: 'Gold', slug: 'gold', category: 'fps', tier: 4, scan_signature: 30, instability: 0.5, resistance: 0.6 },
      { id: 'quantainium_raw', name: 'Quantainium', slug: 'quantainium', category: 'ship', tier: 6, scan_signature: 90, instability: 0.9, resistance: 0.9 }
    ],
    count: 3,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  const LOCS = {
    locations: [
      { id: 'aarON', name: 'Aaron Halo', slug: 'aaron-halo', system: 'Stanton', type: 'exploration_region' },
      { id: 'cellin', name: 'Cellin', slug: 'cellin', system: 'Stanton', type: 'moon' }
    ],
    count: 2,
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
  const LOCATION_ORES = {
    location: { id: 'aarON', name: 'Aaron Halo', system: 'Stanton', type: 'exploration_region' },
    ores: [
      { oreId: 'quantainium_raw', name: 'Quantainium', concentration: 0.4231, tier: 6, tierName: 'Almost Pure', scanSignature: 90 },
      { oreId: 'gold_raw', name: 'Gold', concentration: 0.0812, tier: 4, tierName: 'A Decent Amount', scanSignature: 30 },
      { oreId: 'beryl_raw', name: 'Beryl', concentration: 0.0042, tier: 2, tierName: 'A Trace', scanSignature: 12 }
    ],
    count: 3,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  window.__STRATA_CALLS = [];
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
    if (cmd === 'pin_add') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      return true;
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
        "--remote-debugging-port=9237", "--user-data-dir=/tmp/cdp-m7f2",
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

        # ---- 1) Mining öffnen, Erz-Detail → Karten-Ansicht ----
        await key("F5")
        open_m = await js("!!document.querySelector('.mining')")
        check("f5-opens-mining", bool(open_m))
        await js("[...document.querySelectorAll('.mining .mrow-main')][1].click()")  # Gold
        await asyncio.sleep(0.8)
        cards = int(await js("document.querySelectorAll('.mining .dcard').length"))
        check("detail-cards-4", cards == 4, "cards=" + str(cards))
        dial_px = await js("""JSON.stringify((function(){
            var d=document.querySelector('.mining .dial');
            var r=d?d.getBoundingClientRect():null;
            return r?{w:Math.round(r.width),h:Math.round(r.height)}:null;})())""")
        check("dial-at-least-64px", dial_px != "null" and json.loads(dial_px)["w"] >= 64, dial_px)
        # Zahl+Balken-Zeile ohne Hover sichtbar: 4 Karten × 2 Zeilen, Werte im DOM-Text
        dnums = int(await js("document.querySelectorAll('.mining .dnum').length"))
        check("dnum-rows-8", dnums == 8, "dnums=" + str(dnums))
        num_text = (await js("[...document.querySelectorAll('.mining .dnum')].map(function(e){return e.textContent.trim();}).join('|')") or "")
        check("rank-values-visible", num_text.count("Rank") == 4 and "100 %" in num_text, num_text[:160])
        check("conc-values-visible", num_text.count("Concentration") == 4 and "31.0" in num_text, num_text[:200])
        dbars = int(await js("document.querySelectorAll('.mining .dbar i').length"))
        check("dbar-fills-8", dbars == 8, "dbars=" + str(dbars))
        grid_cols = await js("getComputedStyle(document.querySelector('.mining .dcards')).gridTemplateColumns.split(' ').length")
        check("cards-grid-2cols-wide", str(grid_cols) == "2", "cols=" + str(grid_cols))
        await shot("01-detail-cards-big-dials.png")

        # ---- 2) Ort-Detail: Klick auf Ort-Zeile → strata_location_ores ----
        await key("Escape")
        await asyncio.sleep(0.4)
        loc_tab = await js("[...document.querySelectorAll('.mining .mtab')].find(function(b){return b.textContent.trim().toLowerCase().includes('location')}).click()")
        await asyncio.sleep(0.4)
        await js("window.__STRATA_CALLS.length = 0;")
        await js("document.querySelector('.mining .mlist .lrow .mrow-main').click()")  # Aaron Halo
        await asyncio.sleep(0.8)
        calls = json.loads(await js("JSON.stringify(window.__STRATA_CALLS.map(function(c){return {cmd:c.cmd, loc:c.args.location};}))"))
        check("location-click-invokes-cmd", any(c["cmd"] == "strata_location_ores" for c in calls), json.dumps(calls))
        head = (await js("document.querySelector('.mining .md-title') ? document.querySelector('.mining .md-title').textContent.trim() : 'NONE'"))
        check("loc-detail-title", head == "Aaron Halo", head)  # Fixture: location-ores/aarON (id-Aufruf)
        sub = (await js("document.querySelector('.mining .md-sub') ? document.querySelector('.mining .md-sub').textContent.trim() : 'NONE'"))
        # t_55c06bc0: Wire-Key ist `type` (serde rename) — Subtitle muss system · type zeigen
        check("loc-detail-subtitle-type", sub == "Stanton · exploration_region", sub)
        lines = int(await js("document.querySelectorAll('.mining .lorow-line').length"))
        check("loc-detail-ore-rows-3", lines == 3, "lines=" + str(lines))
        row_txt = (await js("[...document.querySelectorAll('.mining .lorow-line')].map(function(e){return e.textContent.trim();}).join('|')") or "")
        check("loc-detail-conc-sorted-desc", "42.3" in row_txt and "8.1" in row_txt and "0.4" in row_txt, row_txt[:200])
        check("loc-detail-sig-visible", "30" in row_txt and "90" in row_txt, row_txt[:200])
        attr = (await js("document.querySelector('.mining .mattr').textContent")).strip()
        check("attribution-footer-loc-detail", "celd.space" in attr, attr)
        await shot("02-location-detail-ores.png")

        # ---- 3) Back-Pfeil + ESC-Kaskade ----
        await js("document.querySelector('.mining .mback').click()")
        await asyncio.sleep(0.4)
        back = await js("!document.querySelector('.mining .lorow-line') && !!document.querySelector('.mining .mlist')")
        check("loc-detail-back-arrow", bool(back))
        # Enter auf fokussierter Ort-Zeile öffnet ebenfalls
        await js("document.querySelector('.mining .mlist .lrow').focus()")
        await key("Enter")
        await asyncio.sleep(0.8)
        enter_open = int(await js("document.querySelectorAll('.mining .lorow-line').length"))
        check("loc-detail-enter-opens", enter_open == 3, "lines=" + str(enter_open))
        await key("Escape")
        esc1 = await js("!document.querySelector('.mining .lorow-line') && !!document.querySelector('.mining')")
        check("esc-closes-loc-detail", bool(esc1))
        await key("Escape")
        gone = await js("!document.querySelector('.mining')")
        check("esc-exits-mode", bool(gone))

        # ---- 4) pin_add: rows[].idx sind Strings ----
        await key("F5")
        await asyncio.sleep(0.4)
        await js("window.__STRATA_CALLS.length = 0;")
        await js("[...document.querySelectorAll('.mining .mpin')][0].click()")
        await asyncio.sleep(0.5)
        pin = json.loads(await js("JSON.stringify(window.__STRATA_CALLS.find(function(c){return c.cmd==='pin_add'}) || null)"))
        idxs = [r.get("idx") for r in (pin or {}).get("args", {}).get("rows", [])]
        check("pin-add-idx-strings", idxs == ["0", "1", "2"], json.dumps(idxs))

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

asyncio.run(main())
