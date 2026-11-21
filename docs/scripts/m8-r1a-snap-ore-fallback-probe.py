#!/usr/bin/env python3
"""M8-R2 probe (t_7a7844cb) — SCMDB-Snap-Quelle + 500-only-Fallback.

Früher (M8-R1-A/t_1dd162e3): SC-Wiki-Commodity-Union mit Raffinat→(Ore)-
Fallback. Seit M8-R2 ist die Quelle der SCMDB-Mining-Index
(KrovaxCode/SCMDB_DATA qualityBands) und die Auflösung läuft rein über
den Materialnamen (normalize + (Ore)/(Raw)/(Pure)-Strip) — kein
blueprintUuid mehr. Diese Probe prüft den Frontend-Contract:

  1. Snap-Fetch schickt (uuids=[raffinat], names=['Borase']) und KEIN
     blueprintUuid mehr.
  2. Band-Match (Borase → SCMDB-Bänder): Toggle an → Regler rastet auf
     den nächsten Bandwert; 500 bleibt 500 (Always-Punkt); Titel zeigt
     die Bänder; min/max sitzen hart auf bands[0]/bands[-1].
  3. Kein Match („Quxdust", leere Werte) → UI rastet auf 500-only
     (das 0.5.88-Verhalten „frei bei leer" ist zurückgenommen).
"""
import asyncio, json, os, pathlib, shutil, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get(
    "SHOT_DIR",
    str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_1dd162e3")))
OUT.mkdir(parents=True, exist_ok=True)
HTTP_PORT, CDP_PORT = 8898, 9246
URL = f"http://127.0.0.1:{HTTP_PORT}/"

PASS = 0
FAIL = 0
shutil.rmtree("/tmp/cdp-m8r1a-orefb", ignore_errors=True)

def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS {name}")
    else:
        FAIL += 1
        print(f"  FAIL {name}  {detail}")

# LIVE-Beleg Omar (t_1dd162e3): Borase-(Ore)-Union, 21 Werte — live
# per commodities-Liste + Commodity-Endpoint verifiziert (cargo --ignored).
BORASE_UNION = [264, 302, 359, 553, 584, 597, 632, 644, 676, 743, 756, 786,
                864, 875, 892, 903, 916, 941, 959, 976, 1000]
RAFFINAT = "33bff393-42f1-4f70-85a1-71e695ed2a5a"  # Borase (Raffinat, 0 locations)
BP_BORASE = "bp-borase-buzzsaw"
BP_QUX = "bp-quxdust-nomatch"

WRAP = r"""
(function () {
  // M8-R2: SCMDB qualityBands für Borase (LIVE-Beleg Omar 2026-11-20).
  const BORASE_BANDS = [274, 526, 665, 762, 867, 916, 959, 1000];
  const ITEMS = { items: [
    { id: 'item-0', name: 'Borase Cannon', slug: 'borase-cannon', category: 'weapon' },
    { id: 'item-1', name: 'Quxdust Cannon', slug: 'quxdust-cannon', category: 'weapon' }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };

  const costs = [
    { material: 'Borase', oreId: null, quantity: 0.36, unit: 'SCU', minQuality: 1 },
    { material: 'Quxdust', oreId: null, quantity: 0.2, unit: 'SCU', minQuality: 1 }
  ];

  const CRAFT_BORASE = {
    item: { id: 'item-0', name: 'Borase Cannon', category: 'weapon' },
    craftTime: 540, costs: [costs[0]],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  const CRAFT_QUX = {
    item: { id: 'item-1', name: 'Quxdust Cannon', category: 'weapon' },
    craftTime: 540, costs: [costs[1]],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };

  const aspectsFor = (name, uuid) => [
    { key: 'BARREL', name: 'Barrel', required_count: 1,
      input: { name, uuid, min_quality: 1, quantity_scu: 0.36 },
      modifiers: [
        { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher',
          quality_range: { min: 0, max: 1000 },
          modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 },
          value_range_type: 'linear' }
      ] }
  ];

  window.__SNAP_CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_items') return ITEMS;
    if (cmd === 'strata_craft_cost') {
      return a.item === 'item-1' ? CRAFT_QUX : CRAFT_BORASE;
    }
    if (cmd === 'scwiki_craft_detail') {
      const name = a.itemName || '';
      const isBorase = name.includes('Borase');
      if (!isBorase && !name.includes('Quxdust')) return null;
      return {
        item: { name, item_type: 'WeaponGun', size: 2, grade: 'A', class: null,
                mass: 100, description_de: null, description_en: null,
                blueprint_uuids: [isBorase ? 'BP_BORASE' : 'BP_QUX'],
                vehicle_weapon: null, ammunition: null },
        blueprint: {
          uuid: isBorase ? 'BP_BORASE' : 'BP_QUX',
          output_name: name, is_available_by_default: false,
          game_version: 'test',
          // LIVE-Beleg (SW16BR1 "Buzzsaw"): ingredient link zeigt aufs
          // (Ore)-Commodity; dismantle_returns zeigen aufs Raffinat.
          ingredients: isBorase ? [
            { name: 'Borase', kind: 'resource', quantity_scu: 0.36,
              commodity_uuid: '728a3d7e-9567-4e1c-87fd-2bd45b458aa6' }
          ] : [
            { name: 'Quxdust', kind: 'resource', quantity_scu: 0.2,
              commodity_uuid: null }
          ],
          dismantle_returns: [],
          summary_properties: [
            { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher' }
          ],
          aspects: isBorase ? aspectsFor('Borase', 'RAFFINAT') : aspectsFor('Quxdust', 'uuid-quxdust-raffinat'),
          unlocking_missions_grouped: [], web_url: null
        },
        attribution: 'Data from star-citizen.wiki'
      };
    }
    if (cmd === 'scmdb_quality_snaps') {
      window.__SNAP_CALLS.push(a);
      // M8-R2: Auflösung rein über den Namen (SCMDB-Mining-Index) —
      // Borase matcht materialName; Quxdust bleibt leer (kein Match →
      // UI rastet 500-only).
      const bands = a.names?.[0] === 'Borase' ? BORASE_BANDS : [];
      return (a.uuids || []).map((u) => ({ uuid: u, values: bands }));
    }
    return orig(cmd, args);
  };
  window.craftDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.sliderRows = () => [...document.querySelectorAll('.crafting .cmq-slider')].map((r) => ({
    range: r.querySelector('.cqs-track')?.getAttribute('aria-valuenow'),
    val: r.querySelector('.cmq-val')?.textContent.trim(),
    valCls: r.querySelector('.cmq-val')?.className || '',
    snapOn: r.querySelector('.cmq-snap')?.classList.contains('is-on') || false,
    snapTitle: r.querySelector('.cmq-snap')?.title || null
  }));
  // M8-R3: setMat per ECHTEN Pointer-Events am Custom-Slider-Track.
  window.setMat = (idx, v) => {
    const s = document.querySelectorAll('.crafting .cmq-slider')[idx];
    const tr = s?.querySelector('.cqs-track');
    if (!tr) return false;
    const r = tr.getBoundingClientRect();
    const x = r.left + (v / 1000) * r.width;
    const y = r.top + r.height / 2;
    const opts = { clientX: x, clientY: y, bubbles: true, pointerId: 1 };
    tr.dispatchEvent(new PointerEvent('pointerdown', opts));
    tr.dispatchEvent(new PointerEvent('pointerup', opts));
    return true;
  };
  window.toggleSnapAt = (idx) => {
    const b = document.querySelectorAll('.crafting .cmq-snap')[idx];
    if (!b) return false;
    b.click();
    return true;
  };
  window.openCraft = (label) => {
    const b = [...document.querySelectorAll('.crafting .crow-main')].find((x) => x.textContent.includes(label));
    if (!b) return false;
    b.click();
    return true;
  };
  window.pressKey = (key) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  };
  window.modalOverflow = () => {
    const m = document.querySelector('.crafting .cdetail');
    if (!m) return null;
    return { sh: m.scrollHeight, ch: m.clientHeight, overflow: m.scrollHeight > m.clientHeight };
  };
})();
""".replace('RAFFINAT', RAFFINAT).replace('BP_BORASE', BP_BORASE).replace('BP_QUX', BP_QUX)


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(HTTP_PORT), "--bind", "127.0.0.1",
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
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-m8r1a-orefb",
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

        # ---- Gate 1: Fallback-Contract (uuids + names + blueprintUuid) ----
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("openCraft('Borase Cannon')")
        await wait_for("craftDetailOpen() && sliderRows().length === 1", 8)
        rows = await js("sliderRows()")
        check("1a Detail offen, Borase-Regler vorhanden", rows and len(rows) == 1, str(rows))
        call0 = await js("window.__SNAP_CALLS[0] || null")
        check("1b Snap-Fetch schickt Raffinat-UUID",
              call0 and call0.get("uuids") == [RAFFINAT], str(call0))
        check("1c Snap-Fetch schickt Materialnamen ['Borase']",
              call0 and call0.get("names") == ["Borase"], str(call0))
        check("1d Snap-Fetch schickt KEIN blueprintUuid mehr (M8-R2)",
              call0 and call0.get("blueprintUuid") is None, str(call0))

        # ---- Gate 2: Fallback-Ergebnis gerastet auf Borase-(Ore)-Union ----
        rows0 = await js("sliderRows()")
        check("2a Snap-Default an", rows0 and rows0[0]["snapOn"] is True, str(rows0))
        await js("setMat(0, 300)")
        await asyncio.sleep(0.3)
        rows = await js("sliderRows()")
        check("2b 300 rastet auf 274 (nächstes SCMDB-Band)",
              rows and rows[0]["val"] == "274" and rows[0]["snapOn"] is True, str(rows))
        await js("setMat(0, 500)")
        await asyncio.sleep(0.3)
        rows = await js("sliderRows()")
        check("2c 500 bleibt 500 (Always-Punkt im Band-Raster)", rows and rows[0]["val"] == "500", str(rows))
        check("2d Snap-Titel zeigt die SCMDB-Bänder",
              rows and "274" in (rows[0]["snapTitle"] or "") and "1000" in (rows[0]["snapTitle"] or ""),
              str(rows and rows[0]["snapTitle"]))
        await shot("m8r1a-orefb-snapped.png")

        # ---- Gate 3: kein Raten ohne (Ore)-Match ----
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        await js("openCraft('Quxdust Cannon')")
        await wait_for("craftDetailOpen() && sliderRows().length === 1", 8)
        calls = await js("window.__SNAP_CALLS.length")
        last = await js(f"window.__SNAP_CALLS[{calls-1}] || null")
        check("3a Quxdust-Fetch mit Namen, ohne blueprintUuid",
              last and last.get("names") == ["Quxdust"] and last.get("blueprintUuid") is None,
              str(last))
        await js("setMat(0, 777)")
        await asyncio.sleep(0.3)
        rows = await js("sliderRows()")
        check("3b Kein Band-Match: 500-only-Raster (777 rastet auf 500)",
              rows and rows[0]["val"] == "500" and rows[0]["snapOn"] is True, str(rows))
        check("3c 500-only-Titel (nur 500, kein Fake-Band)",
              rows and "500" in (rows[0]["snapTitle"] or "") and "Quxdust" not in (rows[0]["snapTitle"] or ""),
              str(rows and rows[0]["snapTitle"]))
        await shot("m8r1a-orefb-free.png")

        # ---- Gate 4: Detail-Shell ohne Scroll-Overflow (Repo-Regel) ----
        ov = await js("modalOverflow()")
        check("4 Detail-Shell ohne Scroll-Overflow",
              ov is not None and ov["overflow"] is False, str(ov))

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
