#!/usr/bin/env python3
"""M9-L probe (t_4de3c84f) — P6 Battagliaintro-Filter + P1 Craft-ListPicker-Check.

Gates:
  1. P6 Ort-Tab: Battagliaintro (dungeon_v3slotpreset_harvestable_battagliaintro,
     einmalige Mission) taucht NICHT in der Ortsliste auf; andere Dungeon-Orte
     (Aberdeen Caves) bleiben — der dungeon-Typ wird NICHT pauschal gefiltert.
  2. P6 Orte-pro-Erz-Detail: Battagliaintro-Zeile gefiltert, reguläre Zeilen da.
  3. P6 Plan (Mining-Listen): fewest UND concentration wählen Battagliaintro
     nie — obwohl es im Stub die besten Tier/Konzentration-Werte hätte.
  4. P1 Craft-Listen-Umschalter: ListPicker-Dropdown (M9-K P11) auch im
     Crafting-Modus — ECHTER Maus-Klick öffnet Popover, Zeilenwahl wechselt
     (Verifikation, dass M9-K P11 beide Modi abdeckt; kein Button-/Chips-Stil).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Ports 8898/9246 (Probe-Hotspot: pro Task eigene Ports).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_4de3c84f")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8898/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m9l", ignore_errors=True)

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
  // Strata-Fixture: Battagliaintro (dungeon, einmalige Mission) MIT Top-Werten
  // (T6, hohe Konzentration) — würde ohne Filter in BEIDEN Modi gewinnen.
  const ORES = { ores: [
    { id: 'ore-hadanite', name: 'Hadanite', slug: 'hadanite', category: 'ship', tier: 'uncommon',
      scanSignature: 12, instability: 0.3, resistance: 0.4 },
    { id: 'ore-taranite', name: 'Taranite (Raw)', slug: 'taranite-raw', category: 'ship', tier: 'rare',
      scanSignature: 9, instability: 0.5, resistance: 0.2 }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };
  const LOCS = { locations: [
    { id: 'loc-1', name: 'Bashak Fields', slug: 'bashak-fields', system: 'Stanton', type: 'moon' },
    { id: 'loc-2', name: 'Belt Alpha', slug: 'belt-a', system: 'Stanton', type: 'asteroid_field' },
    { id: 'dungeon_v3slotpreset_harvestable_battagliaintro', name: 'Battagliaintro', slug: 'battagliaintro', system: null, type: 'dungeon' },
    { id: 'dungeon_cave_aberdeen_medium', name: 'Aberdeen Caves (Medium)', slug: 'caves-aberdeen-medium', system: null, type: 'dungeon' }
  ], count: 4, attribution: 'Data from celd.space / Strata Mining Tools' };

  const ORE_LOCS = {
    'ore-hadanite': { ore: { id: 'ore-hadanite', name: 'Hadanite' }, sort: 'rank',
      results: { space: [], exploration: [],
        cave: [ { locationId: 'dungeon_v3slotpreset_harvestable_battagliaintro', location: 'Battagliaintro', system: 'Stanton',
                  rankPct: 99, rank: 1, n: 10, concentration: 0.9, tier: 6, tierName: 'Pure' } ],
        surface: [ { locationId: 'loc-1', location: 'Bashak Fields', system: 'Stanton',
                     rankPct: 90, rank: 2, n: 10, concentration: 0.3, tier: 6, tierName: 'Pure' } ] },
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
  // Crafting-Haupttab braucht Daten (M8-R1A-Pitfall) — minimaler Item-Satz.
  const ITEMS = { items: [
    { id: 'it-1', name: 'Test Widget', slug: 'test-widget', category: 'ship',
      craft_time: 60, mats: [ { material: 'Hadanite', unit: 'SCU', quantity: 2, ore_id: 'ore-hadanite' } ] }
  ], count: 1, attribution: 'x' };

  // ---- In-Memory Listen-Engine (kind-aware, Muster m9-c/m9-k) ----
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
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4, scwiki_ready: true };
    if (cmd === 'strata_ores') return ORES;
    if (cmd === 'strata_locations') return LOCS;
    if (cmd === 'strata_items') return ITEMS;
    if (cmd === 'strata_ore_locations') return ORE_LOCS[a.ore] || { results: {}, attribution: 'x' };
    if (cmd === 'scmdb_quality_snaps') return (a.uuids || []).map((u) => ({ uuid: u, values: SNAPS[u] || [] }));
    if (cmd === 'lists_list') return DB.lists.filter((l) => l.kind === a.kind).map((l) => ({
      id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
      item_count: DB.items.filter((i) => i.list_id === l.id).length }));
    if (cmd === 'list_create') {
      const id = DB.seq++;
      DB.lists.push({ id, kind: a.kind, name: a.name, created_at: Math.floor(Date.now() / 1000) });
      return id;
    }
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
  window.bodyText = () => document.body.innerText;

  // Mining: Ort-Tab
  window.locRows = () => qa('.mining .mlist .lrow').map((x) => x.textContent.trim());
  window.clickLocTab = () => { [...qa('.mining .mtab')].find((x) => x.textContent.trim() === 'Locations')?.click(); return true; };
  window.clickOreTab = () => { [...qa('.mining .mtab')].find((x) => x.textContent.trim() === 'Ores')?.click(); return true; };
  window.clickOreRow = (name) => { const r = qa('.mining .mrow').find((x) => x.textContent.includes(name)); (r?.querySelector('.mrow-main') || r)?.click(); return !!r; };
  window.detailCards = () => qa('.mining .dcard-name').map((x) => x.textContent.trim());

  // Mining: Listen-Tab + Plan
  window.listRegion = () => !!q('.mining .mlists');
  window.setListName = (v) => {
    const i = q('.mining .ml-new .ml-input');
    if (!i) return false;
    i.value = v;
    i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.clickNewList = () => { const b = [...document.querySelectorAll('.mining .ml-btn')].find((x) => !x.disabled); b?.click(); return !!b; };
  window.lpLabel = () => q('.mining .lp-btn .lp-label')?.textContent.trim() || '';
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
  window.modeRect = (name) => {
    const b = qa('.mining .ml-modes .ml-mini').find((x) => x.textContent.trim() === name);
    if (!b) return null; const r = b.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
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

  // Crafting: Listen-Tab + ListPicker (P1)
  window.craftOpen = () => !!q('.crafting');
  window.clickCraftListsTab = () => { [...qa('.crafting .ctab')].find((x) => x.textContent.trim() === 'Lists')?.click(); return true; };
  window.craftListRegion = () => !!q('.crafting .clists');
  window.clpLabel = () => q('.crafting .lp-btn .lp-label')?.textContent.trim() || '';
  window.clpPopOpen = () => !!q('.crafting .lp-pop');
  window.clpRows = () => qa('.crafting .lp-pop .lp-row .lp-row-name').map((x) => x.textContent.trim());
  window.clpRect = () => { const b = q('.crafting .lp-btn'); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; };
  window.clpRowRect = (name) => {
    const b = qa('.crafting .lp-pop .lp-row').find((x) => x.querySelector('.lp-row-name')?.textContent.trim() === name);
    if (!b) return null; const r = b.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  };
  // Craft: Neue Liste
  window.csetListName = (v) => {
    const i = q('.crafting .cl-new .cl-input');
    if (!i) return false;
    i.value = v;
    i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.cclickNewList = () => { const b = [...document.querySelectorAll('.crafting .cl-btn')].find((x) => !x.disabled); b?.click(); return !!b; };
  window.chipsLeft = () => qa('.crafting .clists .cchip, .mining .mlists .mchip-list').length;
  window.callsOf = (cmd) => window.__CALLS.filter((c) => c.cmd === cmd);
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
        "--remote-debugging-port=9246", "--user-data-dir=/tmp/cdp-m9l",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9246/json/list", timeout=1) as r:
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

        # ================= P6: Mining — Ort-Tab =================
        await js("window.pressKey('F5')")
        await asyncio.sleep(0.5)
        await wait_for("document.querySelector('.mining')", 8)
        await js("clickLocTab()")
        await wait_for("locRows().length > 0", 8)
        rows = await js("locRows()")
        body = await js("bodyText()")
        check("1a P6 Ort-Tab: Battagliaintro NICHT in der Ortsliste",
              all("Battagliaintro" not in r for r in rows or []), str(rows))
        check("1b P6 Ort-Tab: Aberdeen Caves (dungeon) bleibt", any("Aberdeen" in r for r in rows or []), str(rows))
        check("1c P6: dungeon-Typ nicht pauschal gefiltert (Aberdeen sichtbar)",
              "Aberdeen" in (body or ""), "body")
        await shot("m9l-locations.png")

        # ================= P6: Orte-pro-Erz-Detail =================
        await js("clickOreTab()")
        await wait_for("clickOreRow('Hadanite')", 8)
        await asyncio.sleep(0.4)
        await wait_for("detailCards().length > 0", 8)
        cards = await js("detailCards()")
        check("2a P6 Detail: Battagliaintro-Zeile gefiltert",
              all("Battagliaintro" not in c for c in cards or []), str(cards))
        check("2b P6 Detail: Bashak Fields bleibt",
              any("Bashak" in c for c in cards or []), str(cards))
        await shot("m9l-ore-detail.png")
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)

        # ================= P6: Plan (Mining-Listen, beide Modi) =================
        await js("[...document.querySelectorAll('.mining .mtab')].find(x => x.textContent.trim() === 'Lists')?.click()")
        await wait_for("listRegion()", 8)
        check("3a Liste anlegen", await js("setListName('Run 1')") is True)
        await asyncio.sleep(0.2)
        await js("clickNewList()")
        await wait_for("lpLabel().includes('Run 1')", 8)
        check("3b Add Hadanite Q300", await js("setAdd('had', '250', '300')") is True)
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

        # fewest (Default): Battagliaintro hätte Top-Werte — darf nicht auftauchen.
        check("3c Compute (fewest)", await js("computePlaces()") is True)
        await wait_for("placeRows().length > 0", 10)
        places = await js("placeRows()")
        names = [p["name"] for p in places or []]
        check("3d P6 fewest: Battagliaintro nie im Plan", all("Battagliaintro" not in n for n in names), str(places))
        check("3e fewest: Bashak deckt beide Materialien (Set-Cover)",
              places and len(places) == 1 and names[0] == "Bashak Fields" and len(places[0]["mats"]) == 2,
              str(places))
        await shot("m9l-places-fewest.png")

        # concentration: Battagliaintro hätte als Top-Tier/Tie-Konzentrations-Ort
        # gewonnen (T6 + conc 0.9 vs Bashak T6 conc 0.3) — muss gefiltert sein.
        conc_label = "Top concentration per material"
        # Label via UI-Text ermitteln (i18n-en): 2. Modus-Button.
        conc_label = (await js("[...document.querySelectorAll('.mining .ml-modes .ml-mini')].map(b=>b.textContent.trim())"))
        conc_label = (conc_label or ["", ""])[1]
        check("4a Modus-Button per Maus", await mouse_click(await js(f"modeRect({json.dumps(conc_label)})")) is True)
        await asyncio.sleep(0.3)
        check("4b Compute (concentration)", await js("computePlaces()") is True)
        await wait_for("placeRows().length >= 1", 10)
        places = await js("placeRows()")
        names = [p["name"] for p in places or []]
        check("4c P6 concentration: Battagliaintro nie im Plan", all("Battagliaintro" not in n for n in names), str(places))
        check("4d concentration: Bashak (Hadanite-Top nach Filter) + Belt Alpha (Taranite)",
              "Bashak Fields" in names and "Belt Alpha" in names, str(places))
        await shot("m9l-places-concentration.png")

        # ================= P1: Crafting — ListPicker =================
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        await js("window.pressKey('F6')")
        await wait_for("craftOpen()", 8)
        await asyncio.sleep(0.4)
        check("5a F6 öffnet Crafting-Modus", await js("craftOpen()") is True)
        check("5b Craft strata_items gerufen (Haupttab geladen ohne Crash)",
              (await js("callsOf('strata_items')")) != [])
        await js("clickCraftListsTab()")
        await wait_for("craftListRegion()", 8)
        check("5c P1: Listen-Umschalter ist ListPicker (lp-btn vorhanden)",
              await js("!!document.querySelector('.crafting .lp-btn')") is True)
        check("5d P1: kein Chips-Stil mehr (keine .cchip im Listen-Umschalter)",
              (await js("chipsLeft()")) == 0, str(await js("chipsLeft()")))
        check("5e Craft-Liste anlegen", await js("csetListName('Craft 1')") is True)
        await asyncio.sleep(0.2)
        await js("cclickNewList()")
        await wait_for("clpLabel().includes('Craft 1')", 8)
        check("5f P1: aktive Liste im Dropdown-Label", (await js("clpLabel()")).find("Craft 1") >= 0,
              await js("clpLabel()"))
        check("5g P1: Popover öffnet per ECHTER Maus", await mouse_click(await js("clpRect()")) is True)
        await wait_for("clpPopOpen()", 5)
        rows = await js("clpRows()")
        check("5h P1: Popover listet die Liste", rows and rows == ["Craft 1"], str(rows))
        check("5i P1: zweite Liste anlegen", await js("csetListName('Craft 2')") is True)
        await asyncio.sleep(0.2)
        # Popover noch offen — Esc-Klick außerhalb schließt erst.
        await mouse_click(await js("(function(){const m=document.querySelector('.crafting .cl-head')||document.querySelector('.crafting');const r=m.getBoundingClientRect();return {x:r.x+r.width-30,y:r.y+10}})()"))
        await wait_for("!clpPopOpen()", 5)
        await js("cclickNewList()")
        await wait_for("clpLabel().includes('Craft 2')", 8)
        await mouse_click(await js("clpRect()"))
        await wait_for("clpPopOpen()", 5)
        check("5j P1: beide Listen im Popover", sorted(await js("clpRows()") or []) == ["Craft 1", "Craft 2"],
              str(await js("clpRows()")))
        check("5k P1: Zeile per Maus wählen", await mouse_click(await js("clpRowRect('Craft 1')")) is True)
        await wait_for("!clpPopOpen() && clpLabel().includes('Craft 1')", 5)
        check("5l P1: Wechsel zu Craft 1", (await js("clpLabel()")).find("Craft 1") >= 0)
        await shot("m9l-craft-listpicker.png")

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
