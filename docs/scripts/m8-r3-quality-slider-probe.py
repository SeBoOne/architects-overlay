#!/usr/bin/env python3
"""M8-R3 probe (t_399b10d8) — Custom-Quality-Slider (nativ range → div).

Der native <input type=range> der Material-Regler wird durch
CraftQualitySlider.svelte ersetzt (Track fix 0–1000, Pointer-Events
selbst verwaltet, Thumb nur auf Snap-Punkten). Gates — alle mit ECHTEM
Drag (CDP Input.dispatchMouseEvent), nicht programmatisch:

  R0  Band-Match-Prüfung im Stub: A03 Sniper Rifle (User-Item aus der
      Karte) mit ECHTEN Materialnamen + ECHTEN SCMDB-Bändern (LIVE-
      Beleg 2026-11-21, docs/scripts/m8-r3-band-match-probe.py) —
      Bands-Präsenz im Snap-Titel asserten.
  M1  500 immer optische Mitte: Track fix 0–1000 — Thumb-Mitte bei
      Q500 ist pixel-exakt die Track-Mitte (±1px), auch wenn die
      Band-Grenzen weit daneben liegen (NICHT bands[0]–1000 skaliert).
  B1  Harte Grenze links: Pointer unter bands[0] ziehen → Thumb bleibt
      auf bands[0] (physischer Anschlag, kein Overshoot).
  B2  Harte Grenze rechts: Pointer über bands[-1] → Thumb bleibt 1000.
  H1  Hysterese: Wackeln um 500 (±1px) → Wert bleibt 500 (Anzeige
      springt nicht beim Wackeln um die Mitte).
  H2  Halbweg: von 500 Richtung 521 wechselt der Wert erst jenseits
      des Halbwegs (510.5) → 521; unterhalb des Halbwegs zurück → 500.
      1 unter dem Halbweg (510) bleibt der aktuelle Punkt stehen.
  T1  Ticks: für jeden Snap-Punkt (bands + 500) eine Markierung am
      Track — Anzahl == Anzahl Snap-Punkte.
  K1  Tastatur: ArrowRight/ArrowLeft = nächster/vorheriger Rasterpunkt
      (Snap-an), ±10 (Snap-aus); ARIA (role=slider, valuemin/max/now).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses
File). Bands-Quelle im Stub: LIVE-Bänder aus m8-r3-band-match-probe.py
(mining_data-4.10.3-live.13220821.json).
"""
import asyncio
import base64
import json
import os
import pathlib
import shutil
import urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/cache/scratch/t_399b10d8")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8898/"

shutil.rmtree("/tmp/cdp-m8r3-slider", ignore_errors=True)  # frisches Profil — Snap-Default aus localStorage zurücksetzen

PASS = 0
FAIL = 0


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
  const ITEMS = { items: [
    { id: 'item-0', name: 'Plain Cannon', slug: 'plain-cannon', category: 'weapon' },
    { id: 'item-1', name: 'A03 Sniper Rifle', slug: 'a03-sniper-rifle', category: 'FPS Weapon' }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };

  // M8-R3 User-Item aus der Karte: A03 Sniper Rifle — ECHTE Materialnamen
  // (Strata craft-cost) + ECHTE SCMDB-Bänder (LIVE-Beleg
  // m8-r3-band-match-probe.py, mining_data-4.10.3-live.13220821.json).
  const CRAFT_A03 = {
    item: { id: 'item-1', name: 'A03 Sniper Rifle', category: 'FPS Weapon' },
    craftTime: 900,
    costs: [
      { material: 'Iron', oreId: null, quantity: 0.5, unit: 'SCU', minQuality: null },
      { material: 'Taranite', oreId: null, quantity: 0.2, unit: 'SCU', minQuality: null },
      { material: 'Hadanite', oreId: null, quantity: 0.3, unit: 'SCU', minQuality: null },
      { material: 'Hephaestanite', oreId: null, quantity: 0.4, unit: 'SCU', minQuality: null }
    ],
    blueprintAspects: [
      { key: 'FRAME', name: 'Frame', required_count: 1,
        input: { name: 'Iron', uuid: 'uuid-iron', min_quality: null },
        modifiers: [
          { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher',
            quality_range: { min: 0, max: 1000 },
            modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 },
            value_range_type: 'linear' }
        ] },
      { key: 'BARREL', name: 'Barrel', required_count: 1,
        input: { name: 'Taranite', uuid: 'uuid-taranite', min_quality: null },
        modifiers: [] },
      { key: 'STOCK', name: 'Stock', required_count: 1,
        input: { name: 'Hadanite', uuid: 'uuid-hadanite', min_quality: null },
        modifiers: [] },
      { key: 'SCOPE', name: 'Scope', required_count: 1,
        input: { name: 'Hephaestanite', uuid: 'uuid-hephaestanite', min_quality: null },
        modifiers: [] }
    ],
    blueprintSummary: [
      { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher' }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };

  // ECHTE SCMDB-Bänder (LIVE-Beleg 2026-11-21) — die UI addiert die
  // 500-Mitte selbst (Always-Punkt).
  const SNAPS = {
    'uuid-iron': [325, 521, 664, 796, 874, 917, 953, 1000],
    'uuid-taranite': [310, 525, 646, 718, 853, 925, 957, 1000],
    'uuid-hadanite': [274, 526, 665, 762, 867, 916, 959, 1000],
    'uuid-hephaestanite': [325, 516, 614, 759, 869, 917, 951, 1000]
  };

  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_items') return ITEMS;
    if (cmd === 'strata_craft_cost') return CRAFT_A03;
    if (cmd === 'scwiki_craft_detail') {
      if (!(a.itemName || '').includes('A03')) return null;
      return {
        item: { name: 'A03 Sniper Rifle', item_type: 'FPSWeapon', size: null, grade: null, class: null,
                mass: null, description_de: null, description_en: null,
                blueprint_uuids: ['bp-a03'], vehicle_weapon: null, ammunition: null },
        blueprint: {
          uuid: 'bp-a03', output_name: 'A03 Sniper Rifle', is_available_by_default: false,
          game_version: 'test', dismantle_returns: [],
          summary_properties: CRAFT_A03.blueprintSummary,
          aspects: CRAFT_A03.blueprintAspects,
          unlocking_missions_grouped: [], web_url: null
        },
        attribution: 'Data from star-citizen.wiki'
      };
    }
    if (cmd === 'scmdb_quality_snaps') {
      return (a.uuids || []).map((u) => ({ uuid: u, values: SNAPS[u] || [] }));
    }
    return orig(cmd, args);
  };
  window.craftDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.sliderRows = () => [...document.querySelectorAll('.crafting .cmq-slider')].map((r) => ({
    val: r.querySelector('.cmq-val')?.textContent.trim(),
    valCls: r.querySelector('.cmq-val')?.className || '',
    snapOn: r.querySelector('.cmq-snap')?.classList.contains('is-on') || false,
    snapTitle: r.querySelector('.cmq-snap')?.title || null,
    ticks: r.querySelectorAll('.cqs-tick').length,
    ariaNow: r.querySelector('.cqs-track')?.getAttribute('aria-valuenow'),
    ariaMin: r.querySelector('.cqs-track')?.getAttribute('aria-valuemin'),
    ariaMax: r.querySelector('.cqs-track')?.getAttribute('aria-valuemax')
  }));
  window.openCraft = (label) => {
    const b = [...document.querySelectorAll('.crafting .crow-main')].find((x) => x.textContent.includes(label));
    if (!b) return false;
    b.click();
    return true;
  };
  window.pressKey = (key) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  };
  // M8-R3: Custom-Slider-Geometrie — Track fix 0–1000, Thumb-Mitte = Wert.
  window.cqsGeo = (idx) => {
    const s = document.querySelectorAll('.crafting .cmq-slider')[idx];
    const tr = s?.querySelector('.cqs-track');
    if (!tr) return null;
    const r = tr.getBoundingClientRect();
    const th = s.querySelector('.cqs-thumb')?.getBoundingClientRect();
    return {
      track: { left: r.left, top: r.top, right: r.right, cx: r.left + r.width / 2, w: r.width },
      thumb: th ? { cx: th.left + th.width / 2 } : null,
      now: Number(tr.getAttribute('aria-valuenow'))
    };
  };
  window.pressArrow = (idx, key) => {
    const s = document.querySelectorAll('.crafting .cmq-slider')[idx];
    const tr = s?.querySelector('.cqs-track');
    if (!tr) return false;
    tr.focus();
    tr.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
    return true;
  };
  window.toggleSnapAt = (idx) => {
    const b = document.querySelectorAll('.crafting .cmq-snap')[idx];
    if (!b) return false;
    b.click();
    return true;
  };
})();
"""


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8898", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9247", "--user-data-dir=/tmp/cdp-m8r3-slider",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9247/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        raise RuntimeError("CDP page did not come up")

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
                await asyncio.sleep(0.25)
            return None

        async def mouse(x, y, type_, buttons=1):
            # x als FLOAT an CDP: int()-Truncation (bis 1px) verschiebt den
            # Rohwert um bis zu ~2.6 Einheiten (bei 376px Track) — das bricht
            # die Halbweg-Prüfungen (511 vs 510). CDP akzeptiert Floats.
            await call("Input.dispatchMouseEvent", {
                "type": type_, "x": float(x), "y": float(y), "button": "left",
                "buttons": buttons if type_ in ("mousePressed", "mouseMoved") else 0,
                "clickCount": 1 if type_ == "mousePressed" else 0})

        async def wrap(expr):
            return ok(await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB + "\n" + expr}))

        await wrap(WRAP)
        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("openCraft('A03 Sniper Rifle')")
        await wait_for(
            "craftDetailOpen() && document.querySelectorAll('.crafting .cmq-slider').length >= 4", 8)

        # ---- R0: Band-Match-Prüfung im Stub (User-Item A03) ----
        rows = await js("sliderRows()")
        check("R0 A03: 4 Regler-Zeilen gerendert", rows and len(rows) == 4, str(rows and len(rows)))
        check("R0a Snap-Default an (alle 4)",
              rows and all(r["snapOn"] for r in rows), str(rows))
        # A03-Materialien müssen Bands haben — Reihenfolge = Kosten-Reihenfolge:
        # Iron, Taranite, Hadanite, Hephaestanite (LIVE-Beleg 2026-11-20).
        titles = [r["snapTitle"] or "" for r in (rows or [])]
        for i, (m, first) in enumerate([("Iron", 325), ("Taranite", 310), ("Hadanite", 274), ("Hephaestanite", 325)]):
            t = titles[i] if i < len(titles) else ""
            check(f"R0b {m}: Bands im Snap-Titel (>=2 Punkte, startet {first})",
                  f"[{first}," in t and "500" in t, f"row{i} title={t!r}")
        # 500 ist Always-Punkt in jedem Raster.
        check("R0c 500 in jedem Raster (Always-Punkt)",
              rows and all("500" in (r["snapTitle"] or "") for r in rows), str(rows))
        check("R0d Ticks == Anzahl Snap-Punkte (bands + 500)",
              rows and all(r["ticks"] == len((r["snapTitle"] or "").replace("[", "").replace("]", "").split(",")) and r["ticks"] >= 9
                           for r in rows), str([r["ticks"] for r in (rows or [])]))

        # ---- M1: 500 immer optische Mitte (Track fix 0–1000) ----
        geo = await js("cqsGeo(0)")
        check("M1a Track vorhanden", geo is not None and geo["track"]["w"] > 0, str(geo))
        # Thumb-Mitte bei Q500 == Track-Mitte (±1px) — unabhängig von den
        # Band-Grenzen (IRON bands[0]=325 — NICHT bands[0]–1000 skaliert).
        check("M1 500 == Track-Mitte pixel-exakt (±1px)",
              geo and abs(geo["thumb"]["cx"] - geo["track"]["cx"]) <= 1.0,
              f"thumb={geo['thumb']['cx']} track={geo['track']['cx']}")

        # ---- B1/B2: harte Grenzen (Pointer unter bands[0]/über 1000) ----
        # Echter Drag vom aktuellen Thumb (im Track) HINAUS über die Grenze:
        # der Pointer darf den Thumb nicht über die Grenze ziehen.
        async def drag_beyond(value_from, value_to):
            geo = await js("cqsGeo(0)")
            def xf(v):
                return geo["track"]["left"] + (v + 0.25) / 1000.0 * geo["track"]["w"]
            await mouse(xf(value_from), geo["track"]["top"] + 8, "mousePressed")
            await asyncio.sleep(0.05)
            await mouse(xf(value_to), geo["track"]["top"] + 8, "mouseMoved")
            await asyncio.sleep(0.05)
            await mouse(xf(value_to), geo["track"]["top"] + 8, "mouseReleased")
            await asyncio.sleep(0.2)
        await drag_beyond(500, -60)  # Pointer weit unter bands[0]=325
        geo = await js("cqsGeo(0)")
        expect_x = geo["track"]["left"] + 325 / 1000.0 * geo["track"]["w"]
        check("B1 Pointer unter bands[0] → Thumb bleibt auf bands[0]",
              geo and geo["now"] == 325 and abs(geo["thumb"]["cx"] - expect_x) <= 1.5,
              f"now={geo['now']} thumb={geo['thumb']['cx']} expect={expect_x}")
        await drag_beyond(325, 1060)  # Pointer weit über bands[-1]=1000
        geo = await js("cqsGeo(0)")
        expect_x = geo["track"]["left"] + 1000 / 1000.0 * geo["track"]["w"]
        check("B2 Pointer über bands[-1] → Thumb bleibt 1000",
              geo and geo["now"] == 1000 and abs(geo["thumb"]["cx"] - expect_x) <= 1.5,
              f"now={geo['now']} thumb={geo['thumb']['cx']} expect={expect_x}")

        # ---- H1: Jitter um die Mitte → Wert bleibt 500 ----
        geo = await js("cqsGeo(0)")
        cx = geo["track"]["cx"]
        await mouse(cx, geo["track"]["top"] + 8, "mousePressed")
        await asyncio.sleep(0.05)
        for dx in (-1, 1, -1, 1, -1, 1):
            await mouse(cx + dx, geo["track"]["top"] + 8, "mouseMoved")
            await asyncio.sleep(0.04)
        await mouse(cx, geo["track"]["top"] + 8, "mouseReleased")
        await asyncio.sleep(0.2)
        geo = await js("cqsGeo(0)")
        check("H1 Wackeln um die Mitte → Wert bleibt 500 (JITTER-sicher)",
              geo and geo["now"] == 500, f"now={geo['now']}")

        # ---- H2: Halbweg zwischen 500 und 521 (Iron-Raster) → 521 ----
        # Iron-Raster: [325, 500, 521, 664, …] — Halbweg 500↔521 = 510.5.
        # GEOMETRIE JE DRAG FRISCH LESEN: die %-Abweichungen (.cmq-changes)
        # ändern ihre Textbreite mit dem Wert → Track-left/width verschieben
        # sich; x_for aus einer alten Geometrie würde daneben liegen.
        async def drag_to(v):
            geo = await js("cqsGeo(0)")
            def x_for(vv):
                return geo["track"]["left"] + (vv + 0.25) / 1000.0 * geo["track"]["w"]
            await mouse(x_for(v), geo["track"]["top"] + 8, "mousePressed")
            await asyncio.sleep(0.05)
            await mouse(x_for(v), geo["track"]["top"] + 8, "mouseMoved")
            await asyncio.sleep(0.05)
            await mouse(x_for(v), geo["track"]["top"] + 8, "mouseReleased")
            await asyncio.sleep(0.2)
        await drag_to(505)
        geo = await js("cqsGeo(0)")
        check("H2a Deutlich unter dem Halbweg (505 < 510.5) → bleibt 500",
              geo and geo["now"] == 500, f"now={geo['now']}")
        await drag_to(510)
        geo = await js("cqsGeo(0)")
        check("H2b 1 unter dem Halbweg (510 < 510.5) → bleibt 500 (kein Sprung)",
              geo and geo["now"] == 500, f"now={geo['now']}")
        await drag_to(511)
        geo = await js("cqsGeo(0)")
        check("H2c 1 über dem Halbweg (511 > 510.5) → 521",
              geo and geo["now"] == 521, f"now={geo['now']}")
        await drag_to(511)
        geo = await js("cqsGeo(0)")
        check("H2d Nochmal 511 (von 521 aus) → bleibt 521 (Hysterese relativ zum Punkt)",
              geo and geo["now"] == 521, f"now={geo['now']}")
        await drag_to(510)
        geo = await js("cqsGeo(0)")
        check("H2e Unter denselben Halbweg (510 < 510.5) → zurück auf 500",
              geo and geo["now"] == 500, f"now={geo['now']}")

        # ---- K1: Tastatur + ARIA ----
        await js("pressArrow(0, 'ArrowRight')")
        await asyncio.sleep(0.2)
        geo = await js("cqsGeo(0)")
        check("K1a ArrowRight → nächster Rasterpunkt (521)",
              geo and geo["now"] == 521, f"now={geo['now']}")
        await js("pressArrow(0, 'ArrowLeft')")
        await asyncio.sleep(0.2)
        geo = await js("cqsGeo(0)")
        check("K1b ArrowLeft → vorheriger Rasterpunkt (500)", geo and geo["now"] == 500, f"now={geo['now']}")
        aria = await js("""(() => {
          const tr = document.querySelectorAll('.crafting .cqs-track')[0];
          return { role: tr?.getAttribute('role'), now: tr?.getAttribute('aria-valuenow'),
                   min: tr?.getAttribute('aria-valuemin'), max: tr?.getAttribute('aria-valuemax'),
                   tabindex: tr?.getAttribute('tabindex') };
        })()""")
        check("K1c ARIA role=slider + valuemin/max/now (hard bounds 325–1000)",
              aria and aria["role"] == "slider" and aria["min"] == "325" and aria["max"] == "1000"
              and aria["now"] is not None, str(aria))
        # Snap-aus → Tastatur ±10 frei.
        await js("toggleSnapAt(0)")
        await asyncio.sleep(0.2)
        await js("pressArrow(0, 'ArrowRight')")
        await asyncio.sleep(0.2)
        geo = await js("cqsGeo(0)")
        check("K1d Snap-aus: ArrowRight = ±10 (510)",
              geo and geo["now"] == 510, f"now={geo['now']}")
        await js("pressArrow(0, 'ArrowLeft')")
        await asyncio.sleep(0.2)
        geo = await js("cqsGeo(0)")
        check("K1e Snap-aus: ArrowLeft = ±10 (500)", geo and geo["now"] == 500, f"now={geo['now']}")

        data = ok(await call("Page.captureScreenshot", {"type": "png"}))["data"]
        (OUT / "m8-r3-slider-live.png").write_bytes(base64.b64decode(data))
        print(f"  shot {OUT / 'm8-r3-slider-live.png'}")

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
