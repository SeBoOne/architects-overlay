#!/usr/bin/env python3
"""CDP-Probe für t_c3367dfb (M7-F4: Mining-Feedback-Runde 4).

Belegt auf der echten Svelte-Oberfläche (Tauri-IPC gestubbt, Fixture-Daten,
Live-Contract camelCase + Rarity-Strings):
  1. P4 Grid-Dichte: Erz-Detail-Karten — 4 Spalten bei breiter Shell
     (3440-Viewport, 70vw-Shell → Detailspalte ~2076px), 2 Spalten schmal.
  2. P5 6-Segment-Balken: je Karte 6 GLEICHE Segmente (.segbar i ×6),
     gefüllt = Tier-Stufe (1..6), nicht kontinuierlich.
  3. P3 Ort-Detail: Erz-Liste als Karten (.locard) mit Kürzel-Kachel
     (1.25:1, Rarity-Border), Sig-Wert, 6-Segment-Balken — und Pin-Toggle
     pro Karte.
  4. P1/P2 Sammel-Signatur-Pin: pin_add/update mit Content-Typ
     „scan-signatures", entries [{oreId,label,value,rarity}] (dedupe nach
     oreId), rows[].idx Strings, kpi null; Pin-Box rendert pro Erz EINEN
     Chip (sigs-row, M7-F4: Rarity-Border um die F3-Zeile Kürzel+Basis+×1–×5) und KEINEN Kopf-KPI (kein Doppel-Render des Werts);
     Legacy „scan-signature"-Pin ebenso ohne Doppel-KPI.
Screenshots nach docs/screens/m7-f4-*.png.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screens/m7-f4-pin-polish")))
OUT.mkdir(parents=True, exist_ok=True)
PORT = 8797
URL = f"http://127.0.0.1:{PORT}/"
CDP_PORT = 9241

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

WRAP = r"""
(function () {
  window.__m7f4 = { calls: [], overlay: __OVERLAY__ };
  const ORES = {
    ores: [
      { id: 'beryl_raw', name: 'Beryl', slug: 'beryl', category: 'ship', tier: 'common', scanSignature: 12, instability: 0.4, resistance: 0.3 },
      { id: 'gold_raw', name: 'Gold', slug: 'gold', category: 'fps', tier: 'rare', scanSignature: 30, instability: 0.5, resistance: 0.6 },
      { id: 'laranite_raw', name: 'Laranite', slug: 'laranite', category: 'ship', tier: 'epic', scanSignature: 55, instability: 0.2, resistance: 0.4 },
      { id: 'quantainium_raw', name: 'Quantainium', slug: 'quantainium', category: 'ship', tier: 'legendary', scanSignature: 90, instability: 0.9, resistance: 0.9 },
      { id: 'tungsten_raw', name: 'Tungsten', slug: 'tungsten', category: 'ship', tier: 'uncommon', scanSignature: 64, instability: 0.1, resistance: 0.2 }
    ],
    count: 5,
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
  const row = (id, name, rankPct, rank, n, conc, tier) => ({
    locationId: id, location: name, system: 'Stanton', rankPct, rank, n,
    concentration: conc, tier,
    tierName: {1:'Barely Any',2:'A Trace',3:'Some',4:'A Decent Amount',5:'Lots',6:'Almost Pure'}[tier]
  });
  const ORE_LOCS = (sort) => ({
    ore: { id: 'gold_raw', name: 'Gold' },
    sort,
    tierLegend: { '1': 'Barely Any', '2': 'A Trace', '3': 'Some', '4': 'A Decent Amount', '5': 'Lots', '6': 'Almost Pure' },
    results: {
      space: [
        row('grin_a', 'Grin Mining Belt Alpha', 100, 1, 12, 0.31, 6),
        row('crus_l2', 'Crusader L2', 88, 2, 9, 0.22, 5),
        row('port_o', 'Port Olisare Orbit', 72, 3, 6, 0.11, 3),
        row('yela_belt', 'Yela Belt Micro', 61, 4, 4, 0.06, 2)
      ],
      surface: [row('cellin', 'Cellin', 82, 5, 7, 0.09, 4)],
      cave: [row('jumptown', 'Jumptown', 54, 6, 4, 0.02, 2)],
      exploration: [row('aarON', 'Aaron Halo', 40, 7, 2, 0.004, 1)]
    },
    attribution: 'Data from celd.space / Strata Mining Tools'
  });
  const LOCATION_ORES = {
    location: { id: 'aarON', name: 'Aaron Halo', system: 'Stanton', type: 'exploration_region' },
    ores: [
      { oreId: 'quantainium_raw', name: 'Quantainium', concentration: 0.4231, tier: 6, tierName: 'Almost Pure', scanSignature: 90 },
      { oreId: 'gold_raw', name: 'Gold', concentration: 0.0812, tier: 4, tierName: 'A Decent Amount', scanSignature: 30 },
      { oreId: 'laranite_raw', name: 'Laranite', concentration: 0.0450, tier: 3, tierName: 'Some', scanSignature: 55 },
      { oreId: 'tungsten_raw', name: 'Tungsten', concentration: 0.0120, tier: 2, tierName: 'A Trace', scanSignature: 64 },
      { oreId: 'beryl_raw', name: 'Beryl', concentration: 0.0042, tier: 1, tierName: 'Barely Any', scanSignature: 12 }
    ],
    count: 5,
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  // ---- Fake-Pins-Store (content-fähig, mit Dedupe-Checks wie pins.rs) ----
  const S = window.__m7f4;
  S.pins = __SEED__;
  const copy = (p) => ({ ...p, rows: (p.rows || []).map((r) => ({ ...r })),
                         content: p.content ? JSON.parse(JSON.stringify(p.content)) : null });
  const orig = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    S.calls.push({ cmd, args: a });
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_ores') return ORES;
    if (cmd === 'strata_locations') return LOCS;
    if (cmd === 'strata_ore_locations') return ORE_LOCS(a.sort === 'concentration' ? 'concentration' : 'rank');
    if (cmd === 'strata_location_ores') return LOCATION_ORES;
    if (cmd === 'overlay_get_state') return S.overlay;
    if (cmd === 'pins_state') {
      return { enabled: true, game_running: true, overlay_visible: S.overlay.visible,
               spotlight_screen: 'DP-3', pins: S.pins.filter((p) => true).map(copy) };
    }
    if (cmd === 'pin_add') {
      if (S.pins.some((p) => p.pinned && p.view_id === a.view_id && p.screen === 'DP-3' && p.box_index === a.box_index)) return false;
      if (a.key && S.pins.some((p) => p.pinned && p.view_id === a.view_id && p.key === a.key)) return false;
      S.pins.push({ view_id: a.view_id, screen: 'DP-3', box_index: a.box_index, opacity: 100,
                    pinned: true, x: 400, y: 200 + S.pins.filter((p) => p.pinned).length * 120,
                    title: a.title, kpi: a.kpi ?? null, rows: a.rows || [], content: a.content ?? null, key: a.key ?? null });
      return true;
    }
    if (cmd === 'pin_update') {
      let hit = false;
      for (const p of S.pins) {
        if (p.pinned && p.view_id === a.view_id && p.box_index === a.box_index) {
          p.title = a.title; p.kpi = a.kpi ?? null; p.rows = a.rows || [];
          p.content = a.content ?? null; if (a.key != null) p.key = a.key;
          hit = true;
        }
      }
      return hit;
    }
    if (cmd === 'pin_remove') {
      let hit = false;
      for (const p of S.pins) {
        if (p.pinned && p.view_id === a.view_id && p.screen === a.screen && p.box_index === a.box_index) { p.pinned = false; hit = true; }
      }
      return hit;
    }
    if (/^(pin_move|pin_set_opacity|pin_unpin|pin_set_scale|pin_resize)$/.test(cmd)) return true;
    const r = await orig(cmd, args);
    if (cmd === 'settings_get' || cmd.startsWith('settings_set') || cmd === 'settings_reset_mode_hotkeys') {
      if (r && typeof r === 'object') r.strata_enabled = true;
    }
    return r;
  };
  // PINS_CHANGED_EVENT manuell dispatchen (Stub feuert keine Events) —
  // gleicher Mechanismus wie die Backend-Emit-Pfade (pins.rs notify_changed).
  window.__pinsChanged = function () {
    (globalThis.__evtListeners?.get('ao:pins-changed') || []).forEach((f) => f({ payload: true }));
  };
})();
"""

def wrap_src(overlay, seed_pins):
    return WRAP.replace("__OVERLAY__", json.dumps(overlay)).replace("__SEED__", json.dumps(seed_pins))


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
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
        "--window-size=3440,1500", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-m7f4",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{CDP_PORT}/json/list", timeout=1) as r:
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

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        async def shot(name, clip=None):
            params = {"format": "png"}
            if clip:
                params["clip"] = clip
            r = ok(await call("Page.captureScreenshot", params))
            data = base64.b64decode(r["data"])
            (OUT / name).write_bytes(data)
            print("  shot:", OUT / name, len(data), "bytes")

        async def metrics(w, h):
            await call("Emulation.setDeviceMetricsOverride",
                       {"width": w, "height": h, "deviceScaleFactor": 1, "mobile": False})
            await asyncio.sleep(0.5)

        VK = {"F5": 116, "Escape": 27, "Enter": 13}
        async def key(k):
            vk = VK[k]
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.4)

        PAL_OPEN = {"visible": True, "game_running": True, "click_through": False}
        await call("Page.enable")
        await call("Runtime.enable")

        # =================================================================
        # A — breite Shell (3440): P4 4-Spalten, P5 Segmente, P3 Ort-Karten,
        #     P1/P2 Sammel-Pin-Toggle-LIVE über die echte UI.
        # =================================================================
        await metrics(3440, 1440)
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": wrap_src(PAL_OPEN, [])})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)
        await key("F5")
        check("a1-mining-open", bool(await js("!!document.querySelector('.mining')")))

        # Erz-Detail (Gold = Zeile 2 alphabetisch) → Karten-Grid
        await js("[...document.querySelectorAll('.mining .mrow-main')][1].click()")
        await asyncio.sleep(0.8)
        cols = int(await js("getComputedStyle(document.querySelector('.mining .dcards')).gridTemplateColumns.split(' ').length"))
        check("a2-wide-shell-4-cols", cols == 4, f"cols={cols}")
        cards = int(await js("document.querySelectorAll('.mining .dcard').length"))
        check("a3-ore-detail-7-cards", cards == 7, f"cards={cards}")
        segs = int(await js("document.querySelectorAll('.mining .dcards .segbar i').length"))
        check("a4-six-segments-per-card", segs == cards * 6, f"segbar-i={segs}")
        fills = await js("""JSON.stringify([...document.querySelectorAll('.mining .dcards .segbar')]
            .map(function(b){return [...b.querySelectorAll('i.fill')].length;}))""")
        # GROUPS-Reihenfolge: space → exploration → cave → surface
        check("a5-fills-equal-tier", json.loads(fills) == [6, 5, 3, 2, 1, 2, 4], fills)
        await shot("01-ore-detail-4col-wide.png")
        await shot("01b-segbar-zoom.png", clip=await js("""(function(){
            var els=[...document.querySelectorAll('.mining .dcard')].slice(0,2);
            var r0=els[0].getBoundingClientRect(), r1=els[1].getBoundingClientRect();
            return {x:Math.min(r0.x,r1.x)-4, y:Math.min(r0.y,r1.y)-4,
                    width:Math.max(r0.right,r1.right)-Math.min(r0.x,r1.x)+8,
                    height:Math.max(r0.bottom,r1.bottom)-Math.min(r0.y,r1.y)+8, scale:2};
        })()"""))

        # P3: Ort-Detail-Karten + Rarity-Border an der Kachel
        await key("Escape")
        await asyncio.sleep(0.4)
        await js("[...document.querySelectorAll('.mining .mtab')].find(function(b){return b.textContent.trim().toLowerCase().includes('location')}).click()")
        await asyncio.sleep(0.4)
        await js("window.__m7f4.calls.length = 0;")
        await js("document.querySelector('.mining .mlist .lrow .mrow-main').click()")
        await asyncio.sleep(0.8)
        locards = int(await js("document.querySelectorAll('.mining .locard').length"))
        check("a6-loc-detail-cards-5", locards == 5, f"locards={locards}")
        table_gone = await js("!document.querySelector('.mining .lorow-line')")
        check("a7-table-gone", bool(table_gone))
        border = await js("""[...document.querySelectorAll('.mining .locard .ltile')]
            .map(function(t){return t.style.borderColor;}).join('|')""")
        # Quantainium legendary #4be08a · Gold rare #9dc451 · Laranite epic
        # #6bd694 · Tungsten uncommon #a8a23e · Beryl common #8b949e
        # computedStyle liefert rgb() — Rarity-Rampe: legendary #4be08a,
        # rare #9dc451, epic #6bd694, uncommon #a8a23e, common #8b949e
        check("a8-rarity-borders",
              border == "rgb(75, 224, 138)|rgb(157, 196, 81)|rgb(107, 214, 148)|rgb(168, 162, 62)|rgb(139, 148, 158)",
              border)
        sigtxt = await js("[...document.querySelectorAll('.mining .locard-sig-v')].map(function(e){return e.textContent.trim();}).join('|')")
        check("a9-loc-sig-values", sigtxt == "90|30|55|64|12", sigtxt)
        locfills = await js("""JSON.stringify([...document.querySelectorAll('.mining .locard .segbar')]
            .map(function(b){return [...b.querySelectorAll('i.fill')].length;}))""")
        check("a10-loc-segbar-fills-tier", json.loads(locfills) == [6, 4, 3, 2, 1], locfills)
        await shot("02-location-detail-cards.png")

        # Pin-Toggle LIVE: Gold-Karte (2. locard) → Sammel-Pin anlegen
        await js("[...document.querySelectorAll('.mining .locard .mpin')][1].click()")
        await asyncio.sleep(0.6)
        add = await js("JSON.stringify(window.__m7f4.calls.find(function(c){return c.cmd==='pin_add';}) || null)")
        addj = json.loads(add) if add and add != "null" else None
        ok_add = addj is not None
        check("b1-pin-add-scan-signatures", ok_add and addj["args"]["content"]["type"] == "scan-signatures", add[:220])
        if ok_add:
            e = addj["args"]["content"]["entries"]
            check("b2-entry-shape-oreid-label-value-rarity",
                  len(e) == 1 and set(e[0]) >= {"oreId", "label", "value", "rarity"}
                  and e[0]["oreId"] == "gold_raw" and e[0]["rarity"] == "rare" and e[0]["value"] == 30, json.dumps(e))
            check("b3-kpi-null", addj["args"]["kpi"] is None, str(addj["args"]["kpi"]))
            check("b4-rows-idx-strings", [r["idx"] for r in addj["args"]["rows"]] == ["0"], str(addj["args"]["rows"]))

        # 2. Karte (Quantainium) → pin_update mit 2 Entries (dedupe oreId)
        await js("[...document.querySelectorAll('.mining .locard .mpin')][0].click()")
        await asyncio.sleep(0.6)
        upd = await js("JSON.stringify(window.__m7f4.calls.filter(function(c){return c.cmd==='pin_update';}).pop() || null)")
        upj = json.loads(upd) if upd and upd != "null" else None
        check("b5-pin-update-2-entries", upj is not None and
              [x["oreId"] for x in upj["args"]["content"]["entries"]] == ["gold_raw", "quantainium_raw"], upd[:260])

        # Toggle zurück (Gold raus) → 1 Entry; dann letzten raus → pin_remove
        await js("[...document.querySelectorAll('.mining .locard .mpin')][1].click()")
        await asyncio.sleep(0.6)
        upd2 = json.loads(await js("JSON.stringify(window.__m7f4.calls.filter(function(c){return c.cmd==='pin_update';}).pop() || null)"))
        check("b6-toggle-off-entry-removed", upd2 is not None and
              [x["oreId"] for x in upd2["args"]["content"]["entries"]] == ["quantainium_raw"], json.dumps(upd2)[:200])
        await js("[...document.querySelectorAll('.mining .locard .mpin')][0].click()")
        await asyncio.sleep(0.6)
        rem = await js("JSON.stringify(window.__m7f4.calls.find(function(c){return c.cmd==='pin_remove';}) || null)")
        check("b7-empty-toggle-removes-pin", rem is not None and rem != "null", rem[:160])

        # Sammel-Pin mit 2 Erzen aufbauen + Rendering in der Pin-Box prüfen
        await js("[...document.querySelectorAll('.mining .locard .mpin')][1].click()")
        await asyncio.sleep(0.4)
        await js("[...document.querySelectorAll('.mining .locard .mpin')][0].click()")
        await asyncio.sleep(0.4)
        await js("window.__pinsChanged()")
        await asyncio.sleep(0.6)
        pins_dom = int(await js("document.querySelectorAll('main .pin').length"))
        check("c1-sammel-pin-rendered", pins_dom == 1, f"pins={pins_dom}")
        chips = int(await js("document.querySelectorAll('main .pin .sigs-row').length"))
        check("c2-two-chips", chips == 2, f"chips={chips}")
        chipvals = await js("[...document.querySelectorAll('main .pin .sigs-base')].map(function(e){return e.textContent.trim();}).join('|')")
        check("c3-value-once-per-ore", chipvals == "30|90", chipvals)
        kpi = await js("document.querySelector('main .pin .pin-kpi') !== null")
        check("c4-no-duplicate-header-kpi", kpi is False, f"pin-kpi present={kpi}")
        chipborder = await js("""[...document.querySelectorAll('main .pin .sigs-row')]
            .map(function(c){return getComputedStyle(c).borderColor;}).join('|')""")
        check("c5-chip-rarity-borders", "rgb(157, 196, 81)" in chipborder and "rgb(75, 224, 138)" in chipborder, chipborder)
        rectinfo = await js("""JSON.stringify((function(){
            var v=document.querySelector('main .pin .sigs-base');
            var c=document.querySelector('main .pin .sigs-row');
            if(!v||!c) return null;
            var rv=v.getBoundingClientRect(), rc=c.getBoundingClientRect();
            return {val: v.textContent, color: getComputedStyle(v).color,
                    vis: getComputedStyle(v).visibility, disp: getComputedStyle(v).display,
                    vrect: {x:rv.x,y:rv.y,w:rv.width,h:rv.height},
                    crect: {x:rc.x,y:rc.y,w:rc.width,h:rc.height}};
        })())""")
        print("  debug sigs-base:", rectinfo)
        await shot("03-sammel-pin-no-duplicate.png")

        # =================================================================
        # B — schmale Shell (2100): 2 Spalten + Legacy-Dedupe (scan-signature)
        # =================================================================
        legacy = {"view_id": "mining", "screen": "DP-3", "box_index": 3, "opacity": 100,
                  "pinned": True, "x": 8, "y": 560,  # links neben 70vw-Shell (M5: Shell gewinnt Überlapp — Probe legt die Box neben sie)
                  "title": "GOLD", "kpi": "30",
                  "rows": [{"idx": "0", "name": "Scan signature", "value": "30"}],
                  "content": {"type": "scan-signature", "label": "Gold", "value": 30, "tier": None, "max": None},
                  "key": "scan-signature:gold_raw"}
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": wrap_src(PAL_OPEN, [legacy])})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)
        await metrics(2100, 1200)
        await key("F5")
        await asyncio.sleep(0.4)
        await js("[...document.querySelectorAll('.mining .mrow-main')][1].click()")
        await asyncio.sleep(0.8)
        cols = int(await js("getComputedStyle(document.querySelector('.mining .dcards')).gridTemplateColumns.split(' ').length"))
        check("d1-narrow-shell-2-cols", cols == 2, f"cols={cols}")
        await shot("04-ore-detail-2col-narrow.png")
        # Legacy scan-signature-Pin: Wert EINMAL (Kopf-KPI ausgeblendet)
        lp = await js("""(function(){
            var p=[...document.querySelectorAll('main .pin')].find(function(x){return x.textContent.indexOf('GOLD')>=0;});
            if(!p) return null;
            return {kpi: !!p.querySelector('.pin-kpi'), value: p.querySelector('.sig-value')?.textContent ?? null,
                    count: p.querySelectorAll('.sig-value').length};
        })()""")
        check("d2-legacy-pin-renders", lp is not None, str(lp))
        check("d3-legacy-no-duplicate-kpi", lp is not None and lp["kpi"] is False and lp["value"] == "30" and lp["count"] == 1, str(lp))
        await shot("05-legacy-pin-dedupe.png")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

asyncio.run(main())
