#!/usr/bin/env python3
"""M8-R3-Nachbesserung probe (t_24961acd) — Snap-Punkte ohne UUID +
Reglerbalken exakt gleich lang.

Gates (Spec Omar, Live-Analyse am echten Fenster):
  1. UUID-loses Blueprint-Payload (reales A03-Cache-Format: `input` ohne
     uuid-Feld) → Snap-Fetch läuft über ALLE Aspekte (names), snapPoints
     landen (Toggle NICHT is-empty), Regler rastet auf SCMDB-Bändern.
     Taranite-Beleg: [310, 525, 646, 718, 853, 925, 957, 1000].
  2. Doppelte Materialnamen (dieselbe Ressource in 2 Aspekten) → keine
     Kollision, Werte identisch (Zuordnung über materialKey).
  3. Alle Reglerbalken einer Blueprint-Liste pixel-gleich: Track-Breite
     ±1px UND 500-Mitte auf derselben x-Position — unabhängig von der
     variablen cmq-changes-Breite (User-P2).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_24961acd")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8899/"  # 8797/8897/8898 kollidieren mit parallelen Worker-Probes

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m8r3-uuidless", ignore_errors=True)

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
    { id: 'item-1', name: 'A03 Quality', slug: 'a03-quality', category: 'weapon' }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };

  const CRAFT_NOBP = {
    item: { id: 'item-0', name: 'Plain Cannon', category: 'weapon' },
    craftTime: 960,
    costs: [
      { material: 'Agricium', oreId: null, quantity: 0.36, unit: 'SCU', minQuality: 500 }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };

  // A03-artig: 4 Materialien (Iron/Taranite/Hadanite/Hephaestanite) + Hadanite
  // DOPPELT (dieselbe Ressource in 2 Aspekten). input OHNE uuid-Feld —
  // exakt das reale alte-Cache-Payload (Omar, Live-Analyse).
  const CRAFT_BP = {
    item: { id: 'item-1', name: 'A03 Quality', category: 'weapon' },
    craftTime: 540,
    costs: [
      { material: 'Iron', oreId: null, quantity: 1.2, unit: 'SCU', minQuality: null },
      { material: 'Taranite', oreId: null, quantity: 0.06, unit: 'SCU', minQuality: null },
      { material: 'Hadanite', oreId: null, quantity: 7, unit: 'items', minQuality: 1 },
      { material: 'Hephaestanite', oreId: null, quantity: 0.3, unit: 'SCU', minQuality: null }
    ],
    blueprintAspects: [
      { key: 'FRAME', name: 'Frame', required_count: 1,
        input: { name: 'Iron', min_quality: null, quantity_scu: 1.2 },
        modifiers: [
          { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher',
            quality_range: { min: 0, max: 1000 },
            modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 },
            value_range_type: 'linear' }
        ] },
      { key: 'BARREL', name: 'Barrel', required_count: 1,
        input: { name: 'Taranite', min_quality: null, quantity_scu: 0.06 },
        modifiers: [
          { property_key: 'weapon_damage', label: 'Impact Force', better_when: 'higher',
            quality_range: { min: 0, max: 1000 },
            modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 },
            value_range_type: 'linear' }
        ] },
      { key: 'EMITTER', name: 'Emitter', required_count: 1,
        input: { name: 'Hadanite', min_quality: 1, quantity: 7 },
        modifiers: [
          { property_key: 'weapon_recoil', label: 'Recoil Smoothness', better_when: 'lower',
            quality_range: { min: 0, max: 1000 },
            modifier_range: { at_min_quality: 1.05, at_max_quality: 0.95 },
            value_range_type: 'linear' }
        ] },
      { key: 'COOLING', name: 'Cooling', required_count: 1,
        input: { name: 'Hephaestanite', min_quality: null, quantity_scu: 0.3 },
        modifiers: [
          { property_key: 'weapon_heat', label: 'Heat Dissipation', better_when: 'higher',
            quality_range: { min: 0, max: 1000 },
            modifier_range: { at_min_quality: 0.92, at_max_quality: 1.08 },
            value_range_type: 'linear' }
        ] },
      { key: 'GUARD', name: 'Guard', required_count: 1,
        input: { name: 'Hadanite', min_quality: null, quantity_scu: 0.4 },
        modifiers: [
          { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher',
            quality_range: { min: 0, max: 1000 },
            modifier_range: { at_min_quality: 0.95, at_max_quality: 1.05 },
            value_range_type: 'linear' }
        ] }
    ],
    blueprintSummary: [
      { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher' },
      { property_key: 'weapon_damage', label: 'Impact Force', better_when: 'higher' }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };

  // M8-R2: SCMDB qualityBands je Material (rohe Bänder — die Union mit
  // 500 macht die UI). Taranite = LIVE-Beleg Omar (A03).
  const BANDS = {
    'Iron': [325, 500, 521, 664, 742, 893, 968, 1000],
    'Taranite': [310, 525, 646, 718, 853, 925, 957, 1000],
    'Hadanite': [274, 526, 665, 762, 867, 916, 959, 1000],
    'Hephaestanite': [298, 534, 612, 705, 841, 905, 948, 1000]
  };

  window.__SNAP_CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_items') return ITEMS;
    if (cmd === 'strata_craft_cost') return a.item === 'item-1' ? CRAFT_BP : CRAFT_NOBP;
    if (cmd === 'scwiki_craft_detail') {
      const name = a.itemName || '';
      if (!name.includes('A03')) return null;
      return {
        item: { name: 'A03 Quality', item_type: 'WeaponGun', size: 2, grade: 'A', class: null,
                mass: 100, description_de: null, description_en: null,
                blueprint_uuids: ['bp-1'], vehicle_weapon: null, ammunition: null },
        blueprint: {
          uuid: 'bp-1', output_name: 'A03 Quality', is_available_by_default: false,
          game_version: 'test', dismantle_returns: [],
          summary_properties: CRAFT_BP.blueprintSummary,
          aspects: CRAFT_BP.blueprintAspects,
          unlocking_missions_grouped: [], web_url: null
        },
        attribution: 'Data from star-citizen.wiki'
      };
    }
    if (cmd === 'scmdb_quality_snaps') {
      // REALES Backend-Verhalten (t_24961acd): POSITIONAL — Auflösung rein
      // über den Namen; uuid wird als Echo (auch leer) zurückgegeben.
      window.__SNAP_CALLS.push({ uuids: a.uuids || [], names: a.names || [] });
      const names = a.names || [];
      return names.map((n, i) => ({ uuid: (a.uuids || [])[i] || '', values: BANDS[n] || [] }));
    }
    return orig(cmd, args);
  };
  window.craftDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.sliderRows = () => [...document.querySelectorAll('.crafting .cmq-slider')].map((r, i) => ({
    val: r.querySelector('.cmq-val')?.textContent.trim(),
    snapOn: r.querySelector('.cmq-snap')?.classList.contains('is-on') || false,
    snapEmpty: r.querySelector('.cmq-snap')?.classList.contains('is-empty') || false,
    snapTitle: r.querySelector('.cmq-snap')?.title || null,
    now: r.querySelector('.cqs-track')?.getAttribute('aria-valuenow'),
    min: r.querySelector('.cqs-track')?.getAttribute('aria-valuemin'),
    max: r.querySelector('.cqs-track')?.getAttribute('aria-valuemax'),
    changes: [...((document.querySelectorAll('.crafting .cmq-changes') || [])[i] || [])
      .querySelectorAll('.cmq-change')].map((c) => c.textContent.trim())
  }));
  // Balken-Gleichheit (User-P2): Track-Breite + 500-Mitte je Zeile.
  window.trackGeo = () => [...document.querySelectorAll('.crafting .cmq-slider')].map((r) => {
    const tr = r.querySelector('.cqs-track')?.getBoundingClientRect();
    return tr ? { w: Math.round(tr.width * 100) / 100, cx: Math.round((tr.left + tr.width / 2) * 100) / 100 } : null;
  });
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
  // M8-R4: Effektzeile ist ein EIGENES Row-Element unter .cmq-slider —
  // voll ausgeschrieben (kein Ellipsis-Cut, kein max-width-Cap).
  window.changesGeo = () => [...document.querySelectorAll('.crafting .cmq-changes')].map((r) => {
    const c = r.getBoundingClientRect();
    const cs = getComputedStyle(r);
    return { cw: Math.round(c.width), wrap: cs.flexWrap, maxw: cs.maxWidth,
             ellipsis: cs.textOverflow, hasOverflow: r.width < r.scrollWidth };
  });
  window.modalOverflow = () => {
    const m = document.querySelector('.crafting .cdetail');
    if (!m) return null;
    return { sh: m.scrollHeight, ch: m.clientHeight };
  };
})();
"""


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8899", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9247", "--user-data-dir=/tmp/cdp-m8r3-uuidless",
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
        await js("openCraft('A03 Quality')")
        await wait_for("craftDetailOpen() && document.querySelectorAll('.crafting .cmq-slider').length === 4", 8)
        rows = await js("sliderRows()")
        check("1a 4 Regler-Zeilen (Iron/Taranite/Hadanite/Hephaestanite)", rows and len(rows) == 4, str(rows))

        # ---- Gate A: UUID-loses Payload → Snap-Fetch über Namen ----
        call0 = await js("window.__SNAP_CALLS[0]")
        check("A1 Snap-Fetch geschickt (1 Call)",
              call0 is not None, str(call0))
        check("A2 uuids alle leer (Input ohne uuid-Feld)",
              call0 and call0["uuids"] and all(u == "" for u in call0["uuids"]), str(call0 and call0["uuids"]))
        check("A3 names = alle 5 Aspekt-Materialien (Hadanite doppelt)",
              call0 and call0["names"] == ["Iron", "Taranite", "Hadanite", "Hephaestanite", "Hadanite"],
              str(call0 and call0["names"]))
        rows = await js("sliderRows()")
        check("A4 KEIN Toggle is-empty (snapPoints gelandet trotz leerer UUID)",
              rows and all(not r["snapEmpty"] for r in rows), str([(r["snapEmpty"]) for r in rows]))
        check("A5 Taranite-Titel zeigt LIVE-Bänder (Union mit 500) [310, 500, 525, 646, …]",
              rows and "310, 500, 525, 646" in (rows[1]["snapTitle"] or ""), str(rows and rows[1]["snapTitle"]))

        # Taranite rastet auf den Live-Bändern (A03-Beleg).
        await js("setMat(1, 777)")
        await asyncio.sleep(0.3)
        rows = await js("sliderRows()")
        check("A6 Taranite rastet 777 → 718 (nächstes Band, trotz leerer UUID)",
              rows and rows[1]["now"] == "718", str(rows and rows[1]))
        check("A7 Taranite hartes Clamp: min=310 (bands[0])",
              rows and rows[1]["min"] == "310" and rows[1]["max"] == "1000", str(rows and rows[1]))

        # ---- Gate B: doppelter Materialname (Hadanite in 2 Aspekten) ----
        check("B1 Hadanite-Titel zeigt Bänder (kein Kollision-Verlust)",
              rows and "274, 500, 526" in (rows[2]["snapTitle"] or ""), str(rows and rows[2]["snapTitle"]))
        await js("setMat(2, 700)")
        await asyncio.sleep(0.3)
        rows = await js("sliderRows()")
        check("B2 Hadanite rastet 700 → 665 (Name-Auflösung auch doppelt)",
              rows and rows[2]["now"] == "665", str(rows and rows[2]))

        # ---- Gate C: Balken exakt gleich lang (User-P2) ----
        # Changes-Breite variiert je Zeile (unterschiedliche Labels/Pcts) —
        # der Track darf davon NICHT abhängen.
        rows = await js("sliderRows()")
        changes_lens = [len(" ".join(r["changes"])) for r in rows]
        check("C1 Änderungs-Breiten variieren wirklich (Test-Szenario nicht trivial)",
              len(set(changes_lens)) >= 2, str(changes_lens))
        await js("setMat(3, 850)")  # Changes-Texte ändern sich → Breite variiert dynamisch
        await asyncio.sleep(0.3)
        geo = await js("trackGeo()")
        widths = [g["w"] for g in geo if g]
        cxs = [g["cx"] for g in geo if g]
        check("C2 4 Track-Geometrien gemessen", len(widths) == 4, str(geo))
        check("C3 Alle Balken pixel-gleich (±1px)",
              max(widths) - min(widths) <= 1.0, str(widths))
        check("C4 500-Mitte auf derselben x-Position (±1px)",
              max(cxs) - min(cxs) <= 1.0, str(cxs))
        check("C5 Balkenbreite plausibel (feste Breite ~19rem; Changes in EIGENER Zeile)",
              all(280 <= w <= 330 for w in widths), str(widths))
        await shot("m8-r3-uuidless-equal-tracks.png")

        # Zeile mit GEÄNDERTEN Changes (850) erneut messen — Gleichheit hält.
        geo2 = await js("trackGeo()")
        w2 = [g["w"] for g in geo2 if g]
        check("C6 Gleichheit stabil nach Wert-Änderung (±1px)",
              max(w2) - min(w2) <= 1.0, str(w2))

        cgeo = await js("changesGeo()")
        check("C7 M8-R4: Effektzeile voll ausgeschrieben — wrap, kein max-width/ellipsis, kein Overflow",
              cgeo and all(c["wrap"] == "wrap" and c["maxw"] == "none" and c["ellipsis"] != "ellipsis"
                           and not c["hasOverflow"] for c in cgeo), str(cgeo))
        print("CHANGES_GEO", cgeo)
        await js("window.pressKey('Escape')")

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
