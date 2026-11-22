#!/usr/bin/env python3
"""M9-K probe (t_db9b234f) — UX-Bündel: P9 Freitext-Label, P10 Optimierungs-
Modi, P11 Listen-Dropdown (Custom, KEIN natives select).

Gates:
  1. P10 UI: `.ml-modes` mit 2 Modus-Buttons (Default 'fewest' is-on);
     die alte T1–T6-Tier-Auswahl (`.ml-tier`) ist weg.
  2. P10 fewest (Set-Cover): ein Ort, der beide Materialien abdeckt.
  3. P10 concentration: je Material der Top-Tier-Ort → hier 2 Orte
     (Hadanite → Bashak T6, Taranite → Belt Alpha T6); Maus-Klick auf den
     Modus-Button.
  4. P10 Qualitäts-Bänder filtern in beiden Modi (Taranite Q900: Bashak T4
     → Band 853 < 900 zählt nicht, nur Belt Alpha T6).
  5. P11 Dropdown: aktive Liste im Button; Öffnen per ECHTER Maus (CDP
     Input.dispatchMouseEvent), Popover zeigt alle Listen, Klick auf eine
     Zeile wechselt, Klick außerhalb schließt. Kein natives <select>.
  6. P9 Inventar: Freitext-Positionen tragen KEIN '(Freitext)'-Label mehr;
     Item/Commodity-Kennzeichnung bleibt.

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Ports 8897/9245 (Probe-Hotspot: pro Task eigene Ports).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_db9b234f")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8897/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m9k-ux", ignore_errors=True)

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
  // Strata-Fixture: Hadanite NUR Bashak (T6); Taranite Bashak (T4) + Belt
  // Alpha (T6). Qualitäten 300 → alle Bänder reichen (fewest vs conc
  // unterscheidbar), Q900-Variante für den Band-Filter-Gate.
  const ORES = { ores: [
    { id: 'ore-hadanite', name: 'Hadanite', slug: 'hadanite', category: 'ship', tier: 'uncommon',
      scanSignature: 12, instability: 0.3, resistance: 0.4 },
    { id: 'ore-taranite', name: 'Taranite (Raw)', slug: 'taranite-raw', category: 'ship', tier: 'rare',
      scanSignature: 9, instability: 0.5, resistance: 0.2 }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };
  const LOCS = { locations: [
    { id: 'loc-1', name: 'Bashak Fields', slug: 'bashak-fields', system: 'Stanton', type: 'moon' },
    { id: 'loc-2', name: 'Belt Alpha', slug: 'belt-a', system: 'Stanton', type: 'asteroid_field' }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };

  const ORE_LOCS = {
    'ore-hadanite': { ore: { id: 'ore-hadanite', name: 'Hadanite' }, sort: 'rank',
      results: { space: [], exploration: [], cave: [],
        surface: [ { locationId: 'loc-1', location: 'Bashak Fields', system: 'Stanton',
                     rankPct: 90, rank: 1, n: 10, concentration: 0.3, tier: 6, tierName: 'Pure' } ] },
      attribution: 'x' },
    'ore-taranite': { ore: { id: 'ore-taranite', name: 'Taranite (Raw)' }, sort: 'rank',
      results: { space: [
                    { locationId: 'loc-2', location: 'Belt Alpha', system: 'Stanton',
                      rankPct: 95, rank: 1, n: 8, concentration: 0.4, tier: 6, tierName: 'Pure' } ],
                  exploration: [], cave: [],
                  surface: [ { locationId: 'loc-1', location: 'Bashak Fields', system: 'Stanton',
                               rankPct: 60, rank: 2, n: 10, concentration: 0.1, tier: 4, tierName: 'Common' } ] },
      attribution: 'x' }
  };
  const SNAPS = {
    'ore-hadanite': [274, 526, 665, 762, 867, 916, 959, 1000],
    'ore-taranite': [310, 525, 646, 718, 853, 925, 957, 1000]
  };

  // ---- Inventar (P9): 1 Freitext-Position + 1 Item-Position + 1 Commodity ----
  const INV = [
    { id: '1', entity_kind: null, entity_id: null, name: 'Hadanite', quality: 800,
      location_id: null, location_name: 'Free Depot', quantity: 5, price: null,
      reservations: [], created_at: 0, updated_at: 0 },
    { id: '2', entity_kind: 'item', entity_id: '900', name: 'Plain Cannon', quality: 700,
      location_id: null, location_name: 'Hangar', quantity: 2, price: null,
      reservations: [], created_at: 0, updated_at: 0 },
    { id: '3', entity_kind: 'commodity', entity_id: '1000', name: 'Gold', quality: 900,
      location_id: null, location_name: 'Heim-Hangar', quantity: 12, price: 4500,
      reservations: [], created_at: 0, updated_at: 0 }
  ];

  // ---- In-Memory Listen-Engine (kind-aware, Muster m9-c) ----
  const DB = { seq: 1, lists: [], items: [], mats: [], done: [] };
  function agg(listId) {
    const map = new Map();
    for (const it of DB.items.filter((i) => i.list_id === listId)) {
      for (const m of DB.mats.filter((mm) => mm.item_row === it.id)) {
        const k = m.material + ':' + m.unit;
        const e = map.get(k) || { material: m.material, unit: m.unit, quantity: 0, min_quality: 0, done_qty: 0 };
        e.quantity += m.quantity * it.copies;
        e.min_quality = Math.max(e.min_quality, m.quality);
        map.set(k, e);
      }
    }
    for (const d of DB.done.filter((d) => d.list_id === listId)) {
      const k = d.material + ':' + d.unit;
      if (map.has(k)) map.get(k).done_qty = d.done_qty;
    }
    return [...map.values()].sort((a, b) => a.material.localeCompare(b.material));
  }
  function detailOf(listId) {
    const l = DB.lists.find((x) => x.id === listId);
    if (!l) throw new Error('lists.err_missing');
    return {
      id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
      items: DB.items.filter((i) => i.list_id === listId).map((i) => ({
        ...i, mats: DB.mats.filter((mm) => mm.item_row === i.id)
      })),
      aggregated: agg(listId)
    };
  }

  window.__CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    window.__CALLS.push({ cmd, args: a });
    if (cmd === 'settings_get' && window.__M9K_LANG === 'de') {
      const cfg = await orig(cmd, args);
      return { ...(cfg || {}), language: 'de' };
    }
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4, scwiki_ready: true };
    if (cmd === 'strata_ores') return ORES;
    if (cmd === 'strata_locations') return LOCS;
    if (cmd === 'strata_ore_locations') return ORE_LOCS[a.ore] || { results: {}, attribution: 'x' };
    if (cmd === 'scmdb_quality_snaps') return (a.uuids || []).map((u) => ({ uuid: u, values: SNAPS[u] || [] }));
    if (cmd === 'inventory_list') return INV.map((r) => ({ ...r, reservations: r.reservations.map((x) => ({ ...x })) }));
    if (cmd === 'inventory_locations') {
      const seen = new Set(); const names = [];
      for (const r of INV) { const k = r.location_name.toLowerCase(); if (!seen.has(k)) { seen.add(k); names.push(r.location_name); } }
      return names;
    }
    if (cmd === 'lists_list') return DB.lists.filter((l) => l.kind === a.kind).map((l) => ({
      id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
      item_count: DB.items.filter((i) => i.list_id === l.id).length }));
    if (cmd === 'list_create') {
      const id = DB.seq++;
      DB.lists.push({ id, kind: a.kind, name: a.name, created_at: Math.floor(Date.now() / 1000) });
      return id;
    }
    if (cmd === 'list_rename') { DB.lists.find((l) => l.id === a.listId).name = a.name; return null; }
    if (cmd === 'list_add_item') {
      const it = a.item;
      if (DB.items.some((i) => i.list_id === a.listId && i.item_id === it.item_id.trim()))
        throw new Error('lists.err_dupe');
      const row = DB.seq++;
      DB.items.push({ id: row, list_id: a.listId, item_id: it.item_id, item_name: it.item_name,
                      craft_time: it.craft_time, copies: it.copies, created_at: 1 });
      for (const m of it.mats) DB.mats.push({ item_row: row, material: m.material.trim(),
        ore_id: m.ore_id, unit: m.unit, quantity: m.quantity, quality: m.quality });
      return row;
    }
    if (cmd === 'list_set_done') {
      DB.done = DB.done.filter((d) => !(d.list_id === a.listId && d.material === a.material && d.unit === a.unit));
      if (a.doneQty != null) DB.done.push({ list_id: a.listId, material: a.material, unit: a.unit, done_qty: a.doneQty });
      return null;
    }
    if (cmd === 'list_detail') return detailOf(a.listId);
    return orig(cmd, args);
  };

  // ---- Probe-Helfer ----
  window.pressKey = (key) => window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  const q = (s) => document.querySelector(s);
  const qa = (s) => [...document.querySelectorAll(s)];
  window.listRegion = () => !!q('.mining .mlists');
  window.setListName = (v) => {
    const i = q('.mining .ml-new .ml-input');
    if (!i) return false;
    i.value = v;
    i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.clickNewList = () => { const b = [...document.querySelectorAll('.mining .ml-btn')].find((x) => !x.disabled); b?.click(); return !!b; };
  // M9-K P11: Dropdown (ListPicker) statt Chips.
  window.lpLabel = () => q('.mining .lp-btn .lp-label')?.textContent.trim() || '';
  window.lpPopOpen = () => !!q('.mining .lp-pop');
  window.lpRows = () => qa('.mining .lp-pop .lp-row .lp-row-name').map((x) => x.textContent.trim());
  window.lpRect = () => { const b = q('.mining .lp-btn'); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; };
  window.lpRowRect = (name) => {
    const b = qa('.mining .lp-pop .lp-row').find((x) => x.querySelector('.lp-row-name')?.textContent.trim() === name);
    if (!b) return null; const r = b.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  };
  // M9-K P10: Modus-Toggle statt Tier-Auswahl.
  window.modeButtons = () => qa('.mining .ml-modes .ml-mini').map((b) => ({ on: b.classList.contains('is-on'), t: b.textContent.trim() }));
  window.modeRect = (name) => {
    const b = qa('.mining .ml-modes .ml-mini').find((x) => x.textContent.trim() === name);
    if (!b) return null; const r = b.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  };
  window.tierChips = () => qa('.mining .ml-tier').length;
  window.setAdd = (mat, qty, quality) => {
    const inputs = [...document.querySelectorAll('.mining .ml-add .ml-input')];
    if (inputs.length < 3) return false;
    const [m, n, qq] = inputs;
    m.value = mat; m.dispatchEvent(new Event('input', { bubbles: true }));
    n.value = qty; n.dispatchEvent(new Event('input', { bubbles: true }));
    if (quality != null) { qq.value = quality; qq.dispatchEvent(new Event('input', { bubbles: true })); }
    return true;
  };
  window.suggestNames = () => [...document.querySelectorAll('.mining .ml-suggest-row .ml-suggest-name')].map((x) => x.textContent.trim());
  window.clickSuggest = (name) => {
    const b = [...document.querySelectorAll('.mining .ml-suggest-row')].find((x) => x.textContent.includes(name));
    if (!b) return false;
    b.click();
    return true;
  };
  window.clickAdd = () => {
    const b = [...document.querySelectorAll('.mining .ml-add .ml-btn')].find((x) => !x.disabled);
    if (!b) return false;
    b.click();
    return true;
  };
  window.computePlaces = () => {
    const b = [...document.querySelectorAll('.mining .ml-places-head .ml-btn')].find((x) => !x.disabled);
    if (!b) return false;
    b.click();
    return true;
  };
  window.placeRows = () => [...document.querySelectorAll('.mining .ml-place')].map((p) => ({
    name: p.querySelector('.ml-place-name')?.textContent.trim(),
    mats: [...p.querySelectorAll('.ml-place-mat')].map((m) => m.textContent.trim())
  }));
  window.uncovered = () => [...document.querySelectorAll('.mining .ml-uncovered .ml-place-mat')].map((m) => m.textContent.trim());
  window.callsOf = (cmd) => window.__CALLS.filter((c) => c.cmd === cmd);
  // P9: Freitext-Label im Inventar.
  window.invSubs = () => qa('.inv-row .sub').map((s) => s.textContent.trim());
  window.bodyText = () => document.body.innerText;
})();
"""

# Das Loc-Fixture oben enthielt versehentlich Unicode — kanonisch benennen:
WRAP = WRAP.replace("Allen光芒 Belt", "Belt Alpha")


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
        "--remote-debugging-port=9245", "--user-data-dir=/tmp/cdp-m9k-ux",
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

        async def mouse_click(pt):
            """ECHTER Maus-Klick über CDP-Input — der Dropdown-Regressionfall."""
            if not pt:
                return False
            for t in ("mousePressed", "mouseReleased"):
                ok(await call("Input.dispatchMouseEvent", {
                    "type": t, "x": pt["x"], "y": pt["y"],
                    "button": "left", "clickCount": 1}))
            return True

        await wrap(WRAP)
        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        # ---- Mining-Modus → Listen-Tab ----
        await js("window.pressKey('F5')")
        await asyncio.sleep(0.5)
        await shot("m9k-after-f5.png")
        await wait_for("document.querySelector('.mining')", 8)
        await shot("m9k-mining-open.png")
        await wait_for("mtabs && document.querySelectorAll('.mining .mtab').length === 3", 8)
        await js("[...document.querySelectorAll('.mining .mtab')].find(x => x.textContent.trim() === 'Lists')?.click()")
        await wait_for("listRegion()", 8)

        # ---- Liste anlegen ----
        check("2a Neue-Liste-Feld", await js("setListName('Run 1')") is True)
        await asyncio.sleep(0.2)
        await js("clickNewList()")
        await wait_for("lpLabel().includes('Run 1')", 8)
        check("2b P11: Dropdown-Button zeigt aktive Liste", (await js("lpLabel()")).find("Run 1") >= 0,
              await js("lpLabel()"))
        check("2c P11: kein natives <select> im Listen-Kopf",
              (await js("!!document.querySelector('.mining .ml-bar select')")) is False)

        # ---- Gate 1: P10 UI — Modus-Toggle, Tier-Auswahl weg (innerhalb detail) ----
        modes = await js("modeButtons()")
        check("1a P10: 2 Modus-Buttons vorhanden", modes and len(modes) == 2, str(modes))
        check("1b P10: Default 'fewest' aktiv",
              modes and modes[0]["on"] and not modes[1]["on"], str(modes))
        check("1c P10: Tier-Auswahl (.ml-tier) entfernt", (await js("tierChips()")) == 0,
              str(await js("tierChips()")))
        await shot("m9k-lists-empty.png")

        # ---- Gate 3: fewest — ein Ort deckt beide Materialien ----
        check("3a Add Hadanite Q300", await js("setAdd('had', '250', '300')") is True)
        await asyncio.sleep(0.2)
        await js("clickSuggest('Hadanite')")
        await asyncio.sleep(0.2)
        await js("clickAdd()")
        await wait_for("document.querySelectorAll('.mining .ml-item').length === 1", 8)
        await js("setAdd('taranite', '100', '300')")
        await asyncio.sleep(0.2)
        await js("clickSuggest('Taranite (Raw)')")
        await asyncio.sleep(0.2)
        await js("clickAdd()")
        await wait_for("document.querySelectorAll('.mining .ml-item').length === 2", 8)
        check("3b Compute (fewest)", await js("computePlaces()") is True)
        await wait_for("placeRows().length > 0", 10)
        places = await js("placeRows()")
        check("3c fewest: 1 Ort deckt beide ab",
              places and len(places) == 1 and places[0]["name"] == "Bashak Fields"
              and len(places[0]["mats"]) == 2, str(places))
        await shot("m9k-places-fewest.png")

        # ---- Gate 4: concentration per ECHTER Maus ----
        conc_label = (await js("modeButtons()"))[1]["t"]
        check("4a P10: Modus-Button per Maus klickbar",
              await mouse_click(await js(f"modeRect({json.dumps(conc_label)})")) is True)
        await asyncio.sleep(0.2)
        modes = await js("modeButtons()")
        check("4b concentration aktiv (is-on)", modes and modes[1]["on"] and not modes[0]["on"], str(modes))
        check("4c Compute (concentration)", await js("computePlaces()") is True)
        await wait_for("placeRows().length === 2", 10)
        places = await js("placeRows()")
        by_name = {p["name"]: p for p in places or []}
        check("4d concentration: Bashak deckt NUR Hadanite (Top-Tier L1 für A)",
              "Bashak Fields" in by_name and by_name["Bashak Fields"]["mats"]
              and len(by_name["Bashak Fields"]["mats"]) == 1
              and "Hadanite" in by_name["Bashak Fields"]["mats"][0], str(places))
        check("4e concentration: Belt Alpha deckt NUR Taranite (Top-Tier T6)",
              "Belt Alpha" in by_name and by_name["Belt Alpha"]["mats"]
              and len(by_name["Belt Alpha"]["mats"]) == 1
              and "Taranite" in by_name["Belt Alpha"]["mats"][0], str(places))
        await shot("m9k-places-concentration.png")

        # ---- Gate 5: P11 Dropdown per echter Maus ----
        check("5a Zweite Liste anlegen", await js("setListName('Run 2')") is True)
        await asyncio.sleep(0.2)
        await js("clickNewList()")
        await wait_for("lpLabel().includes('Run 2')", 8)
        check("5b aktive Liste gewechselt (Label Run 2)", (await js("lpLabel()")).find("Run 2") >= 0,
              await js("lpLabel()"))
        check("5c Popover öffnet per Maus", await mouse_click(await js("lpRect()")) is True)
        await wait_for("lpPopOpen()", 5)
        rows = await js("lpRows()")
        check("5d Popover listet ALLE Listen", rows and sorted(rows) == ["Run 1", "Run 2"], str(rows))
        check("5e Zeile per Maus wählen", await mouse_click(await js("lpRowRect('Run 1')")) is True)
        await wait_for("!lpPopOpen() && lpLabel().includes('Run 1')", 5)
        check("5f Wechsel zu Run 1, Popover zu", (await js("lpLabel()")).find("Run 1") >= 0
              and not (await js("lpPopOpen()")), await js("lpLabel()"))
        # Klick außerhalb schließt das offene Popover.
        await mouse_click(await js("lpRect()"))
        await wait_for("lpPopOpen()", 5)
        pt = await js("(function(){const m=document.querySelector('.mining .ml-agg-head');const r=m.getBoundingClientRect();return {x:r.x+40,y:r.y+10}})()")
        await mouse_click(pt)
        await wait_for("!lpPopOpen()", 5)
        check("5g Klick außerhalb schließt Popover", not (await js("lpPopOpen()")))
        await shot("m9k-dropdown.png")

        # ---- Gate 6: Qualitäts-Bänder filtern (fewest, Q900 in Run 2) ----
        await mouse_click(await js("lpRect()"))
        await wait_for("lpPopOpen()", 5)
        await mouse_click(await js("lpRowRect('Run 2')"))
        await wait_for("lpLabel().includes('Run 2')", 5)
        await js("setAdd('had', '50', '900')")
        await asyncio.sleep(0.2)
        await js("clickSuggest('Hadanite')")
        await asyncio.sleep(0.2)
        await js("clickAdd()")
        await wait_for("document.querySelectorAll('.mining .ml-item').length === 1", 8)
        await js("setAdd('taranite', '50', '900')")
        await asyncio.sleep(0.2)
        await js("clickSuggest('Taranite (Raw)')")
        await asyncio.sleep(0.2)
        await js("clickAdd()")
        await wait_for("document.querySelectorAll('.mining .ml-item').length === 2", 8)
        fewest_label = (await js("modeButtons()"))[0]["t"]
        await mouse_click(await js(f"modeRect({json.dumps(fewest_label)})"))
        await asyncio.sleep(0.2)
        check("6a Compute (fewest, Q900)", await js("computePlaces()") is True)
        await wait_for("placeRows().length === 2", 10)
        places = await js("placeRows()")
        by_name = {p["name"]: p for p in places or []}
        check("6b Band-Filter: Bashak (T4→853<900) deckt Taranite NICHT",
              "Bashak Fields" in by_name and "Hadanite" in by_name["Bashak Fields"]["mats"][0]
              and len(by_name["Bashak Fields"]["mats"]) == 1, str(places))
        check("6c Band-Filter: Taranite Q900 nur über Belt Alpha (T6→1000)",
              "Belt Alpha" in by_name and "Taranite" in by_name["Belt Alpha"]["mats"][0], str(places))
        check("6d SCMDB-Snaps mit ore-ids gerufen",
              (await js("callsOf('scmdb_quality_snaps')")) != [])

        # ---- Gate 7: P9 — Freitext-Label weg (F4 Inventar) ----
        # Modus-Wechsel: erst Escape aus dem Mining-Modus (Muster m9-f).
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        await js("window.pressKey('F4')")
        await wait_for("document.querySelector('.inv-row') || document.querySelector('.inv-list')", 8)
        await asyncio.sleep(0.4)
        subs = await js("invSubs()")
        body = await js("bodyText()")
        check("7a P9: kein '(Freitext)'-Label im Inventar",
              "Freitext" not in body.replace("Als Freitext", "").replace("Freitext =", ""), str(subs))
        check("7b P9: Item-Kennzeichnung bleibt", any("Item" in s for s in subs), str(subs))
        check("7c P9: Commodity-Kennzeichnung bleibt", any("Commodity" in s for s in subs), str(subs))
        await shot("m9k-inventory-p9.png")

        # ---- Gate 8: DE-Parität der Modus-Labels ----
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": "window.__M9K_LANG = 'de';"})
        await js("location.reload()")
        await wait_for("document.querySelector('.rail .rentry')", 15)
        await js("window.pressKey('F5')")
        await wait_for("document.querySelectorAll('.mining .mtab').length === 3", 8)
        await js("[...document.querySelectorAll('.mining .mtab')].find(x => x.textContent.trim() === 'Listen')?.click()")
        await wait_for("listRegion()", 8)
        # Nach dem Reload ist die In-Memory-Engine leer — Liste anlegen, dann
        # ist der Orte-Bereich (innerhalb {#if detail}) sichtbar.
        await js("setListName('DE-Test')")
        await asyncio.sleep(0.2)
        await js("clickNewList()")
        await wait_for("lpLabel().includes('DE-Test')", 8)
        await wait_for("modeButtons().length === 2", 8)
        modes = await js("modeButtons()")
        texts = [m["t"] for m in modes or []]
        check("8a DE: Modus-Labels lokalisiert",
              any("wenige Orte" in t for t in texts) and any("Konzentration" in t for t in texts), str(modes))
        await shot("m9k-de.png")

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
