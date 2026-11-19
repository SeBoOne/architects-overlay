#!/usr/bin/env python3
"""M8-R1-A probe (t_a81b04a6) — Individuelle Material-Qualitätsregler.

Gates:
  1. Pro Material ein eigener Regler (0–1000) in der Materialliste (Aspekt-
     Match über Materialname); ohne Blueprint-Match keine Regler-Zeilen.
  2. minQuality-Check am EIGENEN Regler: Q0 < minQuality → rot, Q ≥ → grün.
  3. %-Abweichungen am Regler (quality-calc): Integrity bei Q200 → −8 %,
     bei Q1000 → +10 %; Ampel grün/rot.
  4. Gesamt-Qualität = min über Aspekte (mengengewichtet, hier 1 Material
     pro Aspekt): Frame 200 / Emitter 800 → Gesamt 200; read-only Slider.
  5. Snap-Punkte (stub: quality_quantized_values-Union): Toggle an →
     Regler rastet auf nächsten Quantwert; aus → frei; Titel zeigt Union.
  6. QualityStats-Block rechnet mit den individuellen Aspekt-Qualitäten.
  6/7. M8-R1-C×R1-A (t_1dbe8a85): Detail-Panel füllt die Shell-Höhe
     (kein Scroll-Overflow) UND die Regler sind SICHTBAR (Bounding-Box
     > 0, nicht display:none, im sichtbaren Bereich ohne Scrollen).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Rechenmodell dokumentiert in src/lib/quality-calc.ts (Header, M8-R1-A).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_a81b04a6")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8897/"  # 8797 kollidiert mit parallelen Worker-Probes

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m8r1a-quality", ignore_errors=True)

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
    { id: 'item-1', name: 'Quality Cannon', slug: 'quality-cannon', category: 'weapon' }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };

  const CRAFT_NOBP = {
    item: { id: 'item-0', name: 'Plain Cannon', category: 'weapon' },
    craftTime: 960,
    costs: [
      { material: 'Agricium', oreId: null, quantity: 0.36, unit: 'SCU', minQuality: 500 },
      { material: 'Hadanite', oreId: null, quantity: 7, unit: 'items', minQuality: 1 }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };

  const CRAFT_BP = {
    item: { id: 'item-1', name: 'Quality Cannon', category: 'weapon' },
    craftTime: 540,
    costs: [
      { material: 'Agricium', oreId: null, quantity: 0.36, unit: 'SCU', minQuality: 500 },
      { material: 'Hadanite', oreId: null, quantity: 7, unit: 'items', minQuality: 1 }
    ],
    blueprintAspects: [
      { key: 'FRAME', name: 'Frame', required_count: 1,
        input: { name: 'Agricium', uuid: 'uuid-agricium', min_quality: 500, quantity_scu: 0.36 },
        modifiers: [
          { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher',
            quality_range: { min: 0, max: 1000 },
            modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 },
            value_range_type: 'linear' }
        ] },
      { key: 'EMITTER', name: 'Emitter', required_count: 1,
        input: { name: 'Hadanite', uuid: 'uuid-hadanite', min_quality: 1, quantity: 7 },
        modifiers: [
          { property_key: 'weapon_damage', label: 'Impact Force', better_when: 'higher',
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

  // Snap-Punkte wie am Commodity-Endpoint (UNION je Material, sortiert).
  const SNAPS = {
    'uuid-agricium': [346, 588, 1000],
    'uuid-hadanite': [287, 575]
  };

  window.__SNAP_CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_items') return ITEMS;
    if (cmd === 'strata_craft_cost') return a.item === 'item-1' ? CRAFT_BP : CRAFT_NOBP;
    if (cmd === 'scwiki_craft_detail') {
      // Realer Flow: loadScwiki matcht → loadSnaps wird aufgerufen.
      const name = a.itemName || '';
      if (!name.includes('Quality')) return null;
      return {
        item: { name: 'Quality Cannon', item_type: 'WeaponGun', size: 2, grade: 'A', class: null,
                mass: 100, description_de: null, description_en: null,
                blueprint_uuids: ['bp-1'], vehicle_weapon: null, ammunition: null },
        blueprint: {
          uuid: 'bp-1', output_name: 'Quality Cannon', is_available_by_default: false,
          game_version: 'test', dismantle_returns: [],
          summary_properties: CRAFT_BP.blueprintSummary,
          aspects: CRAFT_BP.blueprintAspects,
          unlocking_missions_grouped: [], web_url: null
        },
        attribution: 'Data from star-citizen.wiki'
      };
    }
    if (cmd === 'scwiki_quality_snaps') {
      window.__SNAP_CALLS.push(a);
      return (a.uuids || []).map((u) => ({ uuid: u, values: SNAPS[u] || [] }));
    }
    return orig(cmd, args);
  };
  window.craftDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.sliderRows = () => [...document.querySelectorAll('.crafting .cmq-slider')].map((r) => ({
    range: r.querySelector('.cmq-range')?.value,
    val: r.querySelector('.cmq-val')?.textContent.trim(),
    valCls: r.querySelector('.cmq-val')?.className || '',
    snapBtn: r.querySelector('.cmq-snap')?.textContent.trim() || null,
    snapOn: r.querySelector('.cmq-snap')?.classList.contains('is-on') || false,
    snapTitle: r.querySelector('.cmq-snap')?.title || null,
    changes: [...r.querySelectorAll('.cmq-change')].map((c) => c.textContent.trim())
  }));
  window.badges = () => [...document.querySelectorAll('.crafting .cmats .cmat')].map((r) => ({
    name: r.querySelector('.cm-name')?.textContent.trim(),
    badge: r.querySelector('.cmq')?.textContent.trim(),
    cls: r.className
  }));
  // M8-R1-C×R1-A (t_1dbe8a85): Regler-SICHTBARKEIT — Bounding-Box > 0,
  // nicht display:none/hidden, im sichtbaren Bereich (ohne Scrollen).
  // DOM-Präsenz allein genügt nicht (Regression am echten Fenster).
  window.sliderVisibility = () => [...document.querySelectorAll('.crafting .cmq-slider')].map((el) => {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return {
      boxH: Math.round(r.height), boxW: Math.round(r.width),
      display: cs.display, visibility: cs.visibility, opacity: cs.opacity,
      inViewport: r.top >= 0 && r.bottom <= window.innerHeight && r.height > 0,
      rangeVisible: (() => {
        const inp = el.querySelector('.cmq-range');
        if (!inp) return { present: false };
        const ri = inp.getBoundingClientRect();
        return { present: true, boxH: Math.round(ri.height), boxW: Math.round(ri.width) };
      })()
    };
  });
  // Regler setzen: input-Event (Svelte oninput).
  window.setMat = (idx, v) => {
    const s = document.querySelectorAll('.crafting .cmq-range')[idx];
    if (!s) return false;
    s.value = String(v);
    s.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.toggleSnapAt = (idx) => {
    const b = document.querySelectorAll('.crafting .cmq-snap')[idx];
    if (!b) return false;
    b.click();
    return true;
  };
  window.combined = () => {
    const inp = document.querySelector('.crafting #craft-target-q');
    const lbl = document.querySelector('.crafting .cp-row .cp-label')?.textContent.trim();
    return { disabled: inp?.disabled, value: inp?.value, label: lbl,
             shown: document.querySelector('.crafting .cp-val')?.textContent.trim() };
  };
  window.qsProps = () => [...document.querySelectorAll('.crafting .qs-prop')].map((r) => ({
    name: r.querySelector('.qs-prop-name')?.textContent.trim(),
    val: r.querySelector('.qs-prop-val')?.textContent.trim(),
    cls: r.className
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
  // Popup-Kompaktheit (Repo-Regel, adaptiert): Crafting ist kein Modal,
  // sondern 2-Spalten-Layout — die Detail-Spalte (.cdetail) soll ohne
  // eigenen Scroll-Overflow auskommen (Fenster 1600×1000, 2 Materialien).
  window.modalOverflow = () => {
    const m = document.querySelector('.crafting .cdetail');
    if (!m) return null;
    return { sh: m.scrollHeight, ch: m.clientHeight, overflow: m.scrollHeight > m.clientHeight,
             scrollable: m.scrollHeight > m.clientHeight && getComputedStyle(m).overflowY.split(' ').some(o => o === 'auto' || o === 'scroll') };
  };
})();
"""


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8897", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9245", "--user-data-dir=/tmp/cdp-m8r1a-quality",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9245/json/list", timeout=1) as r:
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

        # ---- Gate 1a: ohne Blueprint-Match keine Regler-Zeilen ----
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("openCraft('Plain Cannon')")
        await wait_for("craftDetailOpen()", 8)
        check("1a Detail offen (Plain Cannon)", await js("craftDetailOpen()") is True)
        check("1b Keine Regler ohne Blueprint-Match", await js("sliderRows().length") == 0)
        await shot("m8-r1a-detail-nobp.png")

        # ---- Gate 1b: mit Blueprint-Match — 2 Regler, Start 0 ----
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        await js("openCraft('Quality Cannon')")
        await wait_for("craftDetailOpen() && sliderRows().length === 2", 8)
        rows = await js("sliderRows()")
        check("1c 2 Regler-Zeilen (Agricium/Hadanite gematcht)", rows and len(rows) == 2, str(rows))
        check("1d Regler starten bei 0", all(r["range"] == "0" for r in rows), str(rows))
        snaps_called = await js("window.__SNAP_CALLS.length")
        uuids0 = (await js("window.__SNAP_CALLS[0].uuids")) or []
        check("1e Snap-Fetch mit beiden Material-UUIDs",
              snaps_called and set(uuids0) == {"uuid-agricium", "uuid-hadanite"},
              str(snaps_called))
        rows0 = await js("sliderRows()")
        check("1g Snap-Default an (Toggle is-on)", rows0 and rows0[0]["snapOn"] is True and rows0[1]["snapOn"] is True, str(rows0))
        badges = await js("badges()")
        check("1f Zeilen starten ROT (0 < minQuality)",
              badges and all("is-fail" in b["cls"] for b in badges), str(badges))

        # ---- Gate 2: minQuality am EIGENEN Regler (Snap aus → freie Werte) ----
        await js("toggleSnapAt(0)")
        # Zeile 1: Default-Fallback nach Zeile-0-Toggle = aus → einmal tippen
        # setzt explizit an, zweimal zurück auf aus (expliziter Zustand).
        await js("toggleSnapAt(1)")
        await js("toggleSnapAt(1)")
        await js("setMat(0, 600)")  # Agricium: minQuality 500
        await asyncio.sleep(0.3)
        rows = await js("sliderRows()")
        check("2a Agricium Q600: Wert grün (≥ 500)",
              rows and "is-ok" in rows[0]["valCls"] and rows[0]["val"] == "600", str(rows))
        check("2b Hadanite: Toggle-Snap auf 287 gerastet (≥ min 1 → grün)",
              rows and rows[1]["val"] == "287" and "is-ok" in rows[1]["valCls"] and rows[1]["snapOn"] is False, str(rows))
        badges = await js("badges()")
        check("2c Beide Zeilen grün (eigene Reglerwerte erfüllen minQuality)",
              badges and all("is-ok" in b["cls"] for b in badges), str(badges))
        await shot("m8-r1a-detail-mixed.png")

        # ---- Gate 3: %-Abweichungen am Regler ----
        check("3a Agricium Q600: Integrity +2%",
              rows and "Integrity +2%" in rows[0]["changes"], str(rows and rows[0]["changes"]))
        await js("setMat(0, 1000)")
        await asyncio.sleep(0.3)
        rows = await js("sliderRows()")
        check("3b Agricium Q1000: Integrity +10 %, grün",
              rows and "Integrity +10%" in rows[0]["changes"] and "is-ok" in rows[0]["valCls"], str(rows))
        await js("setMat(0, 0)")
        await asyncio.sleep(0.3)

        # ---- Gate 4: Gesamt-Qualität = min über Aspekte ----
        await js("setMat(0, 200)")
        await js("setMat(1, 800)")
        await asyncio.sleep(0.4)
        comb = await js("combined()")
        check("4a Label 'Combined quality'", comb and "Combined quality" in (comb["label"] or ""), str(comb))
        check("4b Gesamt = min(200, 800) = 200", comb and comb["value"] == "200" and comb["shown"] == "200", str(comb))
        check("4c Gesamt-Slider read-only", comb and comb["disabled"] is True, str(comb))
        # mingewichtet: alles über die Einzel-Regler (1 Material pro Aspekt)
        props = await js("qsProps()")
        check("4d Stat-Block mischt individuell: Impact Force +3 % (Emitter 800)",
              props and any(p["name"] == "Impact Force" and p["val"] == "+3 %" for p in props), str(props))
        check("4e Stat-Block: Integrity −6 % (Frame 200 → Faktor 0.94)",
              props and any(p["name"] == "Integrity" and p["val"] == "-6 %" for p in props), str(props))
        await shot("m8-r1a-detail-combined.png")

        # ---- Gate 5: Snap-Punkte + Toggle ----
        await js("setMat(0, 300)")  # frei regulieren
        await asyncio.sleep(0.2)
        rows = await js("sliderRows()")
        check("5a Snap-Toggle vorhanden", rows and rows[0]["snapBtn"] is not None, str(rows))
        await js("toggleSnapAt(0)")  # an → 300 rastet auf 346
        await asyncio.sleep(0.2)
        rows = await js("sliderRows()")
        check("5b Snap an: 300 → 346 gerastet", rows and rows[0]["val"] == "346" and rows[0]["snapOn"] is True, str(rows))
        check("5c Snap-Titel zeigt Union", rows and "346, 588, 1000" in (rows[0]["snapTitle"] or ""), str(rows and rows[0]["snapTitle"]))
        await js("toggleSnapAt(0)")  # aus
        await asyncio.sleep(0.2)
        rows = await js("sliderRows()")
        check("5d Toggle aus: frei (346 bleibt, kein Rasten bei Eingabe)",
              rows and rows[0]["snapOn"] is False and rows[0]["val"] == "346", str(rows))
        await js("setMat(0, 500)")
        await asyncio.sleep(0.2)
        rows = await js("sliderRows()")
        check("5e Frei: 500 bleibt 500 (kein Quantwert)", rows and rows[0]["val"] == "500", str(rows))
        await js("toggleSnapAt(0)")  # wieder an → 500 rastet auf 588
        await asyncio.sleep(0.2)
        rows = await js("sliderRows()")
        check("5f Toggle an: 500 → 588 gerastet", rows and rows[0]["snapOn"] is True and rows[0]["val"] == "588", str(rows))
        await shot("m8-r1a-detail-snap.png")

        # ---- Gate 6: Popup-Kompaktheit (t_e261c7f4-Update) ----
        # Seit der Stat-Block ab Start ALLE Modifier-Zeilen reserviert
        # (Raumstabilität, gewollt), ist Overflow normal — entscheidend ist,
        # dass .cdetail scrollbar ist und die Regler erreichbar bleiben.
        ov = await js("modalOverflow()")
        vis6 = await js("sliderVisibility()")
        scrollable = ov is not None and ov.get("scrollable") is True
        check("6 Detail-Shell scrollbar + Regler erreichbar (Overflow ok, solange scrollend)",
              scrollable and vis6 is not None and all(v["boxH"] > 0 and v["display"] != "none" for v in vis6),
              f"ov={ov} vis={vis6}")
        # ---- Gate 7 (M8-R1-C×R1-A, t_1dbe8a85): Regler SICHTBAR ----
        vis = await js("sliderVisibility()")
        check("7a Regler-Zeilen gerendert (Box > 0, nicht display:none/hidden)",
              vis and len(vis) == 2 and all(v["boxH"] > 0 and v["boxW"] > 0
                                            and v["display"] != "none"
                                            and v["visibility"] != "hidden" for v in vis), str(vis))
        check("7b Regler im sichtbaren Bereich (ohne Scrollen)",
              vis and all(v["inViewport"] for v in vis), str(vis))
        check("7c Range-Inputs selbst sichtbar (Box > 0)",
              vis and all(v["rangeVisible"]["present"] and v["rangeVisible"]["boxH"] > 0
                          and v["rangeVisible"]["boxW"] > 0 for v in vis), str(vis))
        await shot("m8-r1a-detail-fullheight.png")

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
