#!/usr/bin/env python3
"""CDP-Probe für t_f67dfb85 (M7-K2-Fix: Feldnamen-Mismatch snake↔camel).

Reviewer-Befund t_bb3303d0: Mining.svelte las snake_case (`scan_signature`,
`rank_pct`, `tier_name`, `location_id`, `tier_legend`, `l.kind`), aber die
Tauri-Commands serialisieren die strata_client-DTOs camelCase über die Wire
(crates/strata-client/src/dto.rs, `#[serde(rename_all="camelCase")]` bzw.
`#[serde(rename="type")]`). Diese Probe speist EXAKT die camelCase-Wire ein
(serde_json::to_string-Kontrakt) und prüft auf WERTEBENE (nicht Existenz):

  1. dial-rank-fill-values  == ['100','82','54','40']   (rankPct)
  2. tier-badge-texts       == ['Lots','Barely Any','A Trace','A Decent Amount'] (tierName, GROUPS-Ordnung)
  3. tooltip enthält echte Zahlen (rankPct % + concentration % + tierName)
  4. dial-center-signature  == '30'                      (scanSignature)
  5. Ort-Subzeilen echte `type`-Werte, kein `undefined`  (Location.type)
  6. pin_add-Content        content.scanSignature == 30  (K3-Renderer-Vertrag)

F5-Key-Gate-Flow wie m7-k2-mining-probe.py (Repo-Stub + __probeStrataKey).
Die alte Existenz-Probe (m7-k2-mining-probe.py) bleibt als historischer Beleg.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/m7-k2-wire")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if (detail and not ok) else ""))

# ---- camelCase-Wire-Fixtures (1:1 serde_json::to_string der DTOs) ----------
WRAP = r"""
(function () {
  const ORES = {
    ores: [
      { id: 'beryl_raw', name: 'Beryl', slug: 'beryl', category: 'ship', tier: 2, scanSignature: 1350, instability: 0.4, resistance: 0.3 },
      { id: 'copper_raw', name: 'Copper', slug: 'copper', category: 'ship', tier: 1, scanSignature: 0, instability: 0.1, resistance: 0.2 },
      { id: 'gold_raw', name: 'Gold', slug: 'gold', category: 'fps', tier: 4, scanSignature: 30, instability: 0.5, resistance: 0.6 },
      { id: 'quantainium_raw', name: 'Quantainium', slug: 'quantainium', category: 'ship', tier: 6, scanSignature: 90, instability: 0.9, resistance: 0.9 },
      { id: 'titanium_raw', name: 'Titanium', slug: 'titanium', category: 'ship', tier: 3, scanSignature: null, instability: 0.2, resistance: 0.4 }
    ],
    count: 5,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  const LOCS = {
    locations: [
      { id: 'aarON', name: 'Aaron Halo', slug: 'aaron-halo', system: 'Stanton', type: 'exploration_region' },
      { id: 'cellin', name: 'Cellin', slug: 'cellin', system: 'Stanton', type: 'moon' },
      { id: 'grin_a', name: 'Grin Mining Belt Alpha', slug: 'grin-mining-belt-alpha', system: 'Stanton', type: 'belt' },
      { id: 'jumptown', name: 'Jumptown', slug: 'jumptown', system: 'Stanton', type: 'dungeon' }
    ],
    count: 4,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  const row = (locationId, location, system, rankPct, rank, n, concentration, tier, tierName) => ({
    locationId, location, system, rankPct, rank, n, concentration, tier, tierName
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
        "--remote-debugging-port=9237", "--user-data-dir=/tmp/cdp-m7k2-wire",
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

    try:
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

            VK = {"F5": 116, "Escape": 27}
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

            # Probe-Isolation
            await js("try{localStorage.removeItem('__probeStrataKey')}catch(e){}; location.reload();")
            await asyncio.sleep(2.5)

            # Key setzen → Mining öffnen (camelCase-Wire aktiv)
            await js("try{localStorage.setItem('__probeStrataKey','1')}catch(e){}; location.reload();")
            await asyncio.sleep(2.5)
            await key("F5")
            rows = int(await js("document.querySelectorAll('.mining .mlist .mrow').length"))
            check("ore-list-all-5", rows == 5, "rows=" + str(rows))
            await shot("01-ore-list.png")

            # ---- 1) Pin-Content: scanSignature muss durchgereicht sein (Gold-Liste, Detail später) ----
            await js("window.__STRATA_CALLS.length = 0;")
            await js("[...document.querySelectorAll('.mining .mpin')][2].click()")
            await asyncio.sleep(0.5)
            pin = json.loads(await js("JSON.stringify(window.__STRATA_CALLS.find(function(c){return c.cmd==='pin_add'}) || null)"))
            ok_pin = bool(pin) and pin["args"]["content"]["type"] == "scan-signature"
            check("pin-content-type", ok_pin, json.dumps(pin)[:200] if pin else "no call")
            if ok_pin:
                check("pin-content-signature-30", pin["args"]["content"]["scanSignature"] == 30,
                      json.dumps(pin["args"]["content"])[:200])
                check("pin-kpi-signature", pin["args"]["kpi"] == "30", str(pin["args"]["kpi"]))

            # ---- 2) Detail: WERTEBENE auf camelCase-Wire ----
            await js("[...document.querySelectorAll('.mining .mrow-main')][2].click()")
            await asyncio.sleep(0.8)

            # DOM-Reihenfolge = GROUPS-Reihenfolge: space(100), exploration(40), cave(54), surface(82)
            fills = json.loads(await js(
                "JSON.stringify([...document.querySelectorAll('.mining .dial-rank')].map(function(c){return c.style.getPropertyValue('--p');}))"))
            check("dial-rank-fill-values", fills == ['100', '40', '54', '82'], str(fills))

            # nur Detail-Badges (drow) — die Erz-Liste links nutzt .mtier-badge ebenfalls (Tier-Zahl)
            badges = json.loads(await js(
                "JSON.stringify([...document.querySelectorAll('.mining .drow .mtier-badge')].map(function(e){return e.textContent.trim();}))"))
            # DOM-Reihenfolge = GROUPS: space(Lots), exploration(Barely Any), cave(A Trace), surface(A Decent Amount)
            check("tier-badge-texts", badges == ['Lots', 'Barely Any', 'A Trace', 'A Decent Amount'], str(badges))

            tips = json.loads(await js(
                "JSON.stringify([...document.querySelectorAll('.mining .dtip')].map(function(e){return e.textContent.trim();}))"))
            t0 = tips[0] if tips else ""
            check("tooltip-exact-values", ("100%" in t0) and ("31%" in t0) and ("Lots" in t0), t0)

            sig = (await js("var d=document.querySelector('.mining .dial-sig'); d ? d.textContent : 'NO-EL'")).strip()
            check("dial-center-signature", sig == "30", "sig=" + str(sig))

            legend = int(await js("document.querySelectorAll('.mining .ml-item').length"))
            check("legend-6-bands", legend == 6, "legend=" + str(legend))
            await shot("02-detail-values.png")

            # ---- 3) Esc zurück zur Liste: Ort-Subzeilen echte type-Werte ----
            await key("Escape")
            # Ort-Tab öffnen (Locations-Liste mit system · type)
            await js("[...document.querySelectorAll('.mining .mtab, .mining [role=tab]')].find(function(b){return b.textContent.trim().indexOf('Or')===0 || b.getAttribute('aria-selected')!==null && b.textContent.trim()!=='Ores' && b.getAttribute('aria-selected')==='false';})")
            tabbtn = await js("JSON.stringify([...document.querySelectorAll('.mining button[aria-selected]')].map(function(b){return b.textContent.trim();}))")
            await js("[...document.querySelectorAll('.mining button[aria-selected]')].find(function(b){return b.textContent.trim()!=='Ores'}).click()")
            await asyncio.sleep(0.6)
            subs = json.loads(await js(
                "JSON.stringify([...document.querySelectorAll('.mining .mrow-sub')].map(function(e){return e.textContent.trim();}))"))
            # Subs enthalten system · type — siehe mrow-Template
            check("location-type-rendered", any("moon" in t for t in subs) and any("belt" in t for t in subs),
                  str(subs[:6]))
            check("no-undefined-anywhere",
                  "undefined" not in json.dumps(subs) and "undefined" not in json.dumps(badges)
                  and "undefined" not in json.dumps(tips), "undefined in DOM-Werten")

        print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
        if FAIL:
            print("FAILED:", ", ".join(FAIL))
            raise SystemExit(1)
    finally:
        server.terminate()
        proc.terminate()

asyncio.run(main())
