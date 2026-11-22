#!/usr/bin/env python3
"""M9-C probe (t_dc01f34b) — Mining-Listen + Beste-Orte-Optimierung.

Gates:
  1. Mining-Modus (F5) hat einen dritten Reiter „Lists" (eigener Reiter im
     Modus, Omar-Beschluss — keine F-Taste); Klick zeigt MiningLists.
  2. Liste anlegen (kind=mining) → Chip erscheint (Listen-Engine, M9-B).
  3. Material über Typeahead hinzufügen (Menge/Einheit/Qualität) →
     list_add_item mit copies=Menge; Eintragszeile + Aggregation.
  4. M9-P Button-Consolidierung (t_fb405e8d): EIN Button pro Material-Zeile
     („Add to inventory“) öffnet die Inventar-Maske (Qty = offener Rest
     voreingetragen); Done/Partial/Reset entfallen, kein list_set_done.
  5. Beste Orte: Compute → scmdb_quality_snaps (SCMDB-Bänder) +
     strata_ore_locations (Stubs) → Greedy-Set-Cover rendert wenige Orte
     mit abgedeckten Materialien + uncovered-Rest; Qualitäts-Band-Filter
     wirkt (Ort mit zu niedrigem Band zählt nicht).
  6. Deutsch: Tab-Label „Listen" (i18n-Parität live).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Ports 8898/9246 (Probe-Hotspot: pro Task eigene Ports).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_dc01f34b")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8898/"  # 8797 kollidiert mit parallelen Worker-Probes

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m9c-lists", ignore_errors=True)

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
  // Strata-Referenz: 2 Erze (ship→SCU) + 1 items-Eintrag ist über die
  // Listen-Engine nicht ortbar → uncovered.
  const ORES = { ores: [
    { id: 'ore-hadanite', name: 'Hadanite', slug: 'hadanite', category: 'ship', tier: 'uncommon',
      scanSignature: 12, instability: 0.3, resistance: 0.4 },
    { id: 'ore-taranite', name: 'Taranite (Raw)', slug: 'taranite-raw', category: 'ship', tier: 'rare',
      scanSignature: 9, instability: 0.5, resistance: 0.2 }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };
  const LOCS = { locations: [
    { id: 'loc-1', name: 'Bashak Fields', slug: 'bashak-fields', system: 'Stanton', type: 'moon' },
    { id: 'loc-2', name: 'Allen光芒 Belt', slug: 'belt-a', system: 'Stanton', type: 'asteroid_field' }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };

  // ore-locations: L1 deckt Hadanite (T5) + Taranite (T3, Band zu niedrig
  // für Q900) ab; L2 deckt Taranite (T6 → Band 1000) ab. Erwartung:
  // L1 + L2 = 2 Orte; Taranite Q900 NICHT über L1 (Band-Filter).
  const ORE_LOCS = {
    'ore-hadanite': { ore: { id: 'ore-hadanite', name: 'Hadanite' }, sort: 'rank',
      results: { space: [], exploration: [], cave: [],
        surface: [ { locationId: 'loc-1', location: 'Bashak Fields', system: 'Stanton',
                     rankPct: 90, rank: 1, n: 10, concentration: 0.3, tier: 5, tierName: 'Major' } ] },
      attribution: 'Data from celd.space / Strata Mining Tools' },
    'ore-taranite': { ore: { id: 'ore-taranite', name: 'Taranite (Raw)' }, sort: 'rank',
      results: { space: [
                    { locationId: 'loc-2', location: 'Belt Alpha', system: 'Stanton',
                      rankPct: 95, rank: 1, n: 8, concentration: 0.4, tier: 6, tierName: 'Pure' } ],
                  exploration: [], cave: [],
                  surface: [ { locationId: 'loc-1', location: 'Bashak Fields', system: 'Stanton',
                               rankPct: 60, rank: 2, n: 10, concentration: 0.1, tier: 3, tierName: 'Minor' } ] },
      attribution: 'Data from celd.space / Strata Mining Tools' }
  };
  // Taranite-Bänder (SCMDB live 2026-11-20): Tier 3 → idx round(3/6*8)=4 → 718 < 900.
  const SNAPS = {
    'ore-hadanite': [274, 526, 665, 762, 867, 916, 959, 1000],
    'ore-taranite': [310, 525, 646, 718, 853, 925, 957, 1000]
  };

  // ---- In-Memory Listen-Engine (kind-aware, spiegelt cache::lists) ----
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
    // M9-M/M9-P-Backend-Parität: progress/reached + reserved (Stub hat kein
    // Reservierungs-Set — standalone Mining-Listen reservieren nicht).
    return [...map.values()].map((e) => ({
      ...e,
      reserved_qty: 0, reserved_avg_quality: null,
      progress: e.quantity > 0
        ? Math.min(100, Math.round((e.done_qty / e.quantity) * 10000) / 100) : 0,
      reached: false
    })).sort((a, b) => a.material.localeCompare(b.material));
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
    // DE-Gate: settings_get mit Sprache de (setzt die Probe vor dem Reload).
    if (cmd === 'settings_get' && window.__M9C_LANG === 'de') {
      const cfg = await orig(cmd, args);
      return { ...(cfg || {}), language: 'de' };
    }
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4, scwiki_ready: true };
    if (cmd === 'strata_ores') return ORES;
    if (cmd === 'strata_locations') return LOCS;
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
    if (cmd === 'list_rename') { DB.lists.find((l) => l.id === a.listId).name = a.name; return null; }
    if (cmd === 'list_delete') {
      DB.lists = DB.lists.filter((l) => l.id !== a.listId);
      DB.items = DB.items.filter((i) => i.list_id !== a.listId);
      DB.done = DB.done.filter((d) => d.list_id !== a.listId);
      return null;
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
    if (cmd === 'list_remove_item') {
      DB.mats = DB.mats.filter((m) => m.item_row !== a.itemRow);
      DB.items = DB.items.filter((i) => i.id !== a.itemRow);
      return null;
    }
    if (cmd === 'list_set_copies') { DB.items.find((i) => i.id === a.itemRow).copies = a.copies; return null; }
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
  window.mtabs = () => [...document.querySelectorAll('.mining .mtab')].map((b) => b.textContent.trim());
  window.clickTab = (label) => {
    const b = [...document.querySelectorAll('.mining .mtab')].find((x) => x.textContent.trim() === label);
    if (!b) return false;
    b.click();
    return true;
  };
  const q = (s) => document.querySelector(s);
  window.listRegion = () => !!q('.mining .mlists');
  window.setListName = (v) => {
    const i = q('.mining .ml-new .ml-input');
    if (!i) return false;
    i.value = v;
    i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.clickNewList = () => { const b = [...document.querySelectorAll('.mining .ml-btn')].find((x) => !x.disabled); b?.click(); return !!b; };
  // M9-K (P11): Chips sind ein ListPicker-Dropdown — die Liste erscheint im
  // lp-Button (Label) bzw. als lp-row im Popover.
  window.chips = () => [...document.querySelectorAll('.mining .lp-label')].map((c) => c.textContent.trim());
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
  window.itemRows = () => [...document.querySelectorAll('.mining .ml-item')].map((r) => r.textContent.trim());
  window.aggRows = () => [...document.querySelectorAll('.mining .ml-agg-row')].map((r) => r.textContent.trim());
  // M9-P (t_fb405e8d): EIN Button pro aggregierter Zeile (Add to inventory).
  window.actionBtnCount = () => document.querySelectorAll('.mining .ml-agg-actions > .ml-mini').length;
  window.openInvAt = (idx) => {
    const rows = document.querySelectorAll('.mining .ml-agg-row');
    const b = rows[idx]?.querySelector('.ml-agg-actions > .ml-mini');
    if (!b) return false; b.click(); return true;
  };
  window.invFormVisible = () => !!q('.mining .ml-inv-form');
  window.invQtyValue = () => {
    const ins = [...document.querySelectorAll('.mining .ml-inv-form input')];
    return ins.length >= 3 ? ins[2].value : null;
  };
  window.invSet = (loc, q, qty) => {
    const ins = [...document.querySelectorAll('.mining .ml-inv-form input')];
    if (ins.length < 3) return false;
    const [l, qq, n] = ins;
    l.value = loc; l.dispatchEvent(new Event('input', { bubbles: true }));
    qq.value = q; qq.dispatchEvent(new Event('input', { bubbles: true }));
    if (qty != null) { n.value = qty; n.dispatchEvent(new Event('input', { bubbles: true })); }
    return true;
  };
  window.invSubmit = () => {
    const b = q('.mining .ml-inv-form .ml-btn');
    if (!b) return false; b.click(); return true;
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
  // M9-K (P10): Optimierungs-Modi statt Tier-Chips.
  window.fewestModeOn = () => {
    const b = [...document.querySelectorAll('.mining .ml-modes .ml-mini')].find((x) => x.classList.contains('is-on'));
    return !!b && /fewest|wenigste/i.test(b.textContent);
  };
  window.callsOf = (cmd) => window.__CALLS.filter((c) => c.cmd === cmd);
})();
"""

# Das Loc-Fixture oben enthielt ein versehentliches Unicode-Zeichen —
# kanonisch benennen:
WRAP = WRAP.replace("Allen光芒 Belt", "Belt Alpha")


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
        "--remote-debugging-port=9246", "--user-data-dir=/tmp/cdp-m9c-lists",
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

        await wrap(WRAP)
        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        # ---- Gate 1: dritter Reiter im Mining-Modus ----
        await js("window.pressKey('F5')")
        await wait_for("document.querySelector('.mining')", 8)
        await wait_for("mtabs().length === 3", 8)
        tabs = await js("mtabs()")
        check("1a Mining hat 3 Reiter (Ores/Locations/Lists)", tabs and len(tabs) == 3, str(tabs))
        check("1b Reiter 'Lists' vorhanden (i18n)", tabs and "Lists" in tabs, str(tabs))
        check("1c MiningLists noch nicht aktiv", await js("listRegion()") is False)
        check("1d Tab-Klick öffnet Listen-View", await js("clickTab('Lists')") is True)
        await wait_for("listRegion()", 8)
        check("1e MiningLists gerendert", await js("listRegion()") is True)
        await shot("m9c-tab-lists.png")

        # ---- Gate 2: Liste anlegen (kind=mining) ----
        check("2a Neue-Liste-Feld vorhanden", await js("setListName('FPS-Run')") is True)
        await asyncio.sleep(0.2)
        await js("clickNewList()")
        await wait_for("chips().length === 1", 8)
        chips = await js("chips()")
        check("2b Chip angelegt", chips and "FPS-Run" in chips[0], str(chips))
        create_calls = await js("callsOf('list_create')")
        check("2c kind=mining über die IPC-Grenze",
              create_calls and create_calls[0]["args"]["kind"] == "mining", str(create_calls))

        # ---- Gate 3: Material über Typeahead hinzufügen ----
        check("3a Add-Formular vorhanden", await js("setAdd('had', '250', '800')") is True)
        await asyncio.sleep(0.2)
        sug = await js("suggestNames()")
        check("3b Typeahead filtert auf Hadanite", sug == ["Hadanite"], str(sug))
        await js("clickSuggest('Hadanite')")
        await asyncio.sleep(0.2)
        check("3c Eintrag übernommen (Feld = Name)", await js("clickAdd()") is True)
        await wait_for("itemRows().length === 1", 8)
        rows = await js("itemRows()")
        check("3d Eintragszeile Hadanite", rows and "Hadanite" in rows[0] and "250" in rows[0], str(rows))
        add_calls = await js("callsOf('list_add_item')")
        it = add_calls[-1]["args"]["item"]
        check("3e copies=Menge 250, Einheit SCU (ship), ore_id gesetzt",
              it["copies"] == 250 and it["mats"][0]["unit"] == "SCU"
              and it["mats"][0]["ore_id"] == "ore-hadanite" and it["mats"][0]["quantity"] == 1
              and it["mats"][0]["quality"] == 800, str(it))
        agg = await js("aggRows()")
        check("3f Aggregation 0/250, Q ≥ 800", agg and "0/250" in agg[0] and "800" in agg[0], str(agg))

        # Zweites Material: Taranite mit Q900 (Band-Filter-Gate vorbereiten)
        await js("setAdd('taranite', '100', '900')")
        await asyncio.sleep(0.2)
        await js("clickSuggest('Taranite (Raw)')")
        await asyncio.sleep(0.2)
        await js("clickAdd()")
        await wait_for("itemRows().length === 2", 8)
        await shot("m9c-entries.png")

        # ---- Gate 4: M9-P Button-Consolidierung (t_fb405e8d) ----
        # EIN Button pro Material-Zeile („Add to inventory") — Done/Partial/
        # Reset entfallen, der Status ergibt sich aus done+reserved (M9-M).
        # Formular öffnen: Qty = offener Rest voreingetragen (P9: kein
        # Qty-Feld mehr in der Zeile), bestätigen → inventory_add
        # (standalone Liste, KEINE Reservierung, KEIN list_set_done).
        check("4a Genau EIN Action-Button pro Zeile", await js("actionBtnCount()") == 2, str(await js("actionBtnCount()")))
        check("4b Add to inventory öffnet das Formular", await js("openInvAt(0)") is True)
        await wait_for("invFormVisible()", 5)
        prefill = await js("invQtyValue()")
        check("4c Qty voreingetragen = offener Rest (250)", prefill == "250", str(prefill))
        await js("invSet('Depot', '500', '150')")
        await asyncio.sleep(0.2)
        await js("invSubmit()")
        await asyncio.sleep(0.5)
        agg = await js("aggRows()")
        check("4d Teil-Add: Zeile bleibt 0/250 (kein list_set_done)",
              agg and "0/250" in agg[0], str(agg))
        add_calls = await js("callsOf('inventory_add')")
        done_calls = await js("callsOf('list_set_done')")
        check("4e inventory_add gerufen, list_set_done entfallen",
              add_calls != [] and done_calls == [], f"add={add_calls} done={done_calls}")
        await shot("m9c-manage.png")

        # ---- Gate 5: Beste Orte (Set-Cover + Band-Filter) ----
        # M9-K (P10): Optimierungs-Modi statt Tier-Chips — 'fewest' aktiv.
        check("5a Modus-Buttons vorhanden, 'fewest' aktiv", await js("fewestModeOn()") is True,
              str(await js("fewestModeOn()")))
        check("5b Compute-Button", await js("computePlaces()") is True)
        await wait_for("placeRows().length > 0", 10)
        places = await js("placeRows()")
        names = [p["name"] for p in places or []]
        check("5c Beide Orte gewählt (keiner deckt alles ab)",
              names and sorted(names) == ["Bashak Fields", "Belt Alpha"], str(places))
        # Band-Filter: Taranite Q900 NUR über Belt Alpha (T6); Bashak (T3 → 718)
        # darf Taranite NICHT abdecken.
        bashak = next((p for p in places or [] if p["name"] == "Bashak Fields"), None)
        alpha = next((p for p in places or [] if p["name"] == "Belt Alpha"), None)
        check("5d Qualitäts-Band-Filter: Bashak deckt NUR Hadanite ab",
              bashak is not None and len(bashak["mats"]) == 1
              and "Hadanite" in bashak["mats"][0], str(bashak))
        check("5e Belt Alpha deckt Taranite ab (Band 1000 ≥ 900)",
              alpha is not None and any("Taranite" in m for m in alpha["mats"]), str(alpha))
        check("5f Kein uncovered-Rest", (await js("uncovered()")) == [], str(await js("uncovered()")))
        snaps = await js("callsOf('scmdb_quality_snaps')")
        check("5g SCMDB-Snaps mit Strata-ore-ids gerufen",
              snaps and set(snaps[0]["args"]["uuids"]) == {"ore-hadanite", "ore-taranite"}, str(snaps))
        await shot("m9c-places.png")

        # ---- Gate 6: Deutsch (Sprache über settings_get-Stub, Reload) ----
        await call("Page.addScriptToEvaluateOnNewDocument", {
            "source": "window.__M9C_LANG = 'de';"
        })
        await js("location.reload()")
        await wait_for("document.querySelector('.rail .rentry')", 15)
        await js("window.pressKey('F5')")
        await wait_for("mtabs().length === 3", 8)
        tabs_de = await js("mtabs()")
        check("6a DE: Reiter 'Listen'", tabs_de and "Listen" in tabs_de, str(tabs_de))
        check("6b DE: Listen-View öffnet", await js("clickTab('Listen')") is True)
        await wait_for("listRegion()", 8)
        # Nach dem Reload ist die In-Memory-Engine leer — Liste anlegen, dann
        # ist der Beste-Orte-Bereich (innerhalb {#if detail}) sichtbar.
        await js("setListName('DE-Test')")
        await asyncio.sleep(0.2)
        await js("clickNewList()")
        await wait_for("chips().length === 1", 8)
        region_text = await js("document.querySelector('.mining .mlists')?.textContent || ''")
        check("6c DE: 'Beste Orte' + 'Beste Orte berechnen' beschriftet",
              "Beste Orte" in (region_text or ""), str(region_text)[:200])
        await shot("m9c-de.png")

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
