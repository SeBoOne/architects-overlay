#!/usr/bin/env python3
"""M9-O probe (t_a39041f8) — ore_id-Resolution + Qualitäts-Fallback.

Gates (Feedback P5, Diagnose Omar 2026-11-21):
  1. Liste anlegen + Einträge:
     a) „Glacosite" als FREITEXT (kein Suggest-Treffer) → list_add_item mit
        mats[0].ore_id === NULL (Ursache A).
     b) „Riccite" über Suggest → ore_id 'riccite_ore', Q965 (Ursache B).
  2. Compute beste Orte:
     a) strata_ores wird für den ore_id-Fallback gerufen.
     b) Glacosite via Namens-Match „Glacosite (Raw)" aufgelöst → Ort
        „Glacier Shelf" gerendert, NICHT uncovered.
     c) Riccite: kein Ort erfüllt Q965 (beide Orte SEKUNDÄR → max. 743 =
        bands[3], Sebos Regel M9-O R2) → Fallback-Ort „Ric Highlands" (T4)
        MIT Warn-Hinweis
        place_below_quality, Material zählt als covered, NICHT uncovered.
  3. Deutsch: Warn-Text live auf Deutsch (i18n-Parität).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Ports 8899/9247 (Probe-Hotspot: pro Task eigene Ports).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_a39041f8")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8899/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m9o-places", ignore_errors=True)

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
  // Strata-Katalog: Glacosite heißt „Glacosite (Raw)" (Namens-Match-Case),
  // Riccite trägt ore_id 'riccite_ore' (Real-Beleg Omar).
  const ORES = { ores: [
    { id: 'ore-glacosite', name: 'Glacosite (Raw)', slug: 'glacosite', category: 'ground_vehicle', tier: 'common',
      scanSignature: 5, instability: 0.2, resistance: 0.3 },
    { id: 'riccite_ore', name: 'Riccite', slug: 'riccite', category: 'ground_vehicle', tier: 'uncommon',
      scanSignature: 7, instability: 0.4, resistance: 0.2 }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };
  const LOCS = { locations: [
    { id: 'loc-g1', name: 'Glacier Shelf', slug: 'glacier-shelf', system: 'Stanton', type: 'moon' },
    { id: 'loc-r2', name: 'Ric Lowlands', slug: 'ric-lowlands', system: 'Stanton', type: 'moon' },
    { id: 'loc-r4', name: 'Ric Highlands', slug: 'ric-highlands', system: 'Stanton', type: 'moon' }
  ], count: 3, attribution: 'Data from celd.space / Strata Mining Tools' };

  // Glacosite: T5 → Band 959 ≥ Q500 ✓ (qualifiziert normal).
  // Riccite: beide Orte SEKUNDÄR (rank 2, rankPct < 100) → max.
  // bands[3]=743 < Q965 → Fallback. (M9-O R2: rank 1 = PRIMARY würde
  // 501–1000 liefern und ohne Warnung qualifizieren.)
  const ORE_LOCS = {
    'ore-glacosite': { ore: { id: 'ore-glacosite', name: 'Glacosite (Raw)' }, sort: 'rank',
      results: { space: [], exploration: [], cave: [],
        surface: [ { locationId: 'loc-g1', location: 'Glacier Shelf', system: 'Stanton',
                     rankPct: 90, rank: 1, n: 10, concentration: 0.3, tier: 5, tierName: 'Major' } ] },
      attribution: 'Data from celd.space / Strata Mining Tools' },
    'riccite_ore': { ore: { id: 'riccite_ore', name: 'Riccite' }, sort: 'rank',
      results: { space: [], exploration: [], cave: [],
        surface: [ { locationId: 'loc-r2', location: 'Ric Lowlands', system: 'Stanton',
                     rankPct: 40, rank: 2, n: 10, concentration: 0.1, tier: 2, tierName: 'Minor' },
                   { locationId: 'loc-r4', location: 'Ric Highlands', system: 'Stanton',
                     rankPct: 60, rank: 2, n: 10, concentration: 0.2, tier: 4, tierName: 'Abundant' } ] },
      attribution: 'Data from celd.space / Strata Mining Tools' }
  };
  // Riccite-Bänder = SCMDB-Muster (t_a39041f8-Spec).
  const SNAPS = {
    'ore-glacosite': [274, 526, 665, 762, 867, 916, 959, 1000],
    'riccite_ore': [325, 525, 671, 743, 870, 942, 965, 1000]
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
    // DE-Gate: settings_get mit Sprache de (setzt die Probe vor dem Reload).
    if (cmd === 'settings_get' && window.__M9O_LANG === 'de') {
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
    if (cmd === 'list_set_copies') { DB.items.find((i) => i.id === a.itemRow).copies = a.copies; return null; }
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
  window.setAdd = (mat, qty, quality) => {
    const inputs = [...document.querySelectorAll('.mining .ml-add .ml-input')];
    if (inputs.length < 3) return false;
    const [m, n, qq] = inputs;
    m.value = mat; m.dispatchEvent(new Event('input', { bubbles: true }));
    n.value = qty; n.dispatchEvent(new Event('input', { bubbles: true }));
    if (quality != null) { qq.value = quality; qq.dispatchEvent(new Event('input', { bubbles: true })); }
    return true;
  };
  window.clickAdd = () => {
    const b = [...document.querySelectorAll('.mining .ml-add .ml-btn')].find((x) => !x.disabled);
    if (!b) return false;
    b.click();
    return true;
  };
  window.itemRows = () => [...document.querySelectorAll('.mining .ml-item')].map((r) => r.textContent.trim());
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
})();

// Setup-Ablauf (Liste + Glacosite-Freitext + Riccite-Suggest) einmalig je Sprache.
window.setupList = async function () {
  window.pressKey('F5');
  for (let i = 0; i < 50 && !document.querySelector('.mining .mtab'); i++)
    await new Promise((r) => setTimeout(r, 100));
  const tab = [...document.querySelectorAll('.mining .mtab')]
    .find((x) => x.textContent.trim() === (window.__M9O_LANG === 'de' ? 'Listen' : 'Lists'));
  if (!tab) return 'no-tab';
  tab.click();
  for (let i = 0; i < 50 && !document.querySelector('.mining .mlists'); i++)
    await new Promise((r) => setTimeout(r, 100));
  const nameI = document.querySelector('.mining .ml-new .ml-input');
  nameI.value = 'M9O';
  nameI.dispatchEvent(new Event('input', { bubbles: true }));
  await new Promise((r) => setTimeout(r, 150));
  [...document.querySelectorAll('.mining .ml-btn')].find((x) => !x.disabled)?.click();
  for (let i = 0; i < 50 && !document.querySelector('.mining .ml-add'); i++)
    await new Promise((r) => setTimeout(r, 100));
  // Glacosite als FREITEXT (kein exakter Name → ore_id null, Ursache A).
  const inputs = [...document.querySelectorAll('.mining .ml-add .ml-input')];
  inputs[0].value = 'Glacosite';
  inputs[0].dispatchEvent(new Event('input', { bubbles: true }));
  await new Promise((r) => setTimeout(r, 200));
  inputs[1].value = '10';
  inputs[1].dispatchEvent(new Event('input', { bubbles: true }));
  await new Promise((r) => setTimeout(r, 100));
  [...document.querySelectorAll('.mining .ml-add .ml-btn')].find((x) => !x.disabled)?.click();
  for (let i = 0; i < 50 && document.querySelectorAll('.mining .ml-item').length < 1; i++)
    await new Promise((r) => setTimeout(r, 100));
  // Riccite über Suggest (exakter Treffer → ore_id 'riccite_ore', Ursache B).
  const inputs2 = [...document.querySelectorAll('.mining .ml-add .ml-input')];
  inputs2[0].value = 'ric';
  inputs2[0].dispatchEvent(new Event('input', { bubbles: true }));
  await new Promise((r) => setTimeout(r, 200));
  const sug = [...document.querySelectorAll('.mining .ml-suggest-row')]
    .find((x) => x.textContent.includes('Riccite'));
  sug?.click();
  await new Promise((r) => setTimeout(r, 200));
  const inputs3 = [...document.querySelectorAll('.mining .ml-add .ml-input')];
  inputs3[1].value = '5';
  inputs3[1].dispatchEvent(new Event('input', { bubbles: true }));
  inputs3[2].value = '965';
  inputs3[2].dispatchEvent(new Event('input', { bubbles: true }));
  await new Promise((r) => setTimeout(r, 100));
  [...document.querySelectorAll('.mining .ml-add .ml-btn')].find((x) => !x.disabled)?.click();
  for (let i = 0; i < 50 && document.querySelectorAll('.mining .ml-item').length < 2; i++)
    await new Promise((r) => setTimeout(r, 100));
  return document.querySelectorAll('.mining .ml-item').length;
};
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
        "--remote-debugging-port=9247", "--user-data-dir=/tmp/cdp-m9o-places",
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

        async def wait_for(expr, timeout=15):
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

        # ---- Gate 1: Setup + Freitext-Eintrag ohne ore_id ----
        n_items = await wait_for("window.setupList()", 25)
        check("1a Setup: Liste + 2 Einträge", n_items == 2, str(n_items))
        add_calls = await js("callsOf('list_add_item')") or []
        m0 = add_calls[0]["args"]["item"]["mats"][0]
        check("1b Glacosite-Freitext → ore_id NULL (Ursache A reproduziert)",
              m0["material"] == "Glacosite" and m0["ore_id"] is None, str(add_calls[0]))
        m1 = add_calls[1]["args"]["item"]["mats"][0]
        check("1c Riccite-Suggest → ore_id 'riccite_ore', Q965 (Ursache B)",
              m1["ore_id"] == "riccite_ore" and m1["quality"] == 965, str(add_calls[1]))
        await shot("m9o-entries.png")

        # ---- Gate 2: Compute — ore_id-Resolution + Qualitäts-Fallback ----
        check("2a Compute-Button", await js("computePlaces()") is True)
        await wait_for("placeRows().length > 0", 12)
        places = await js("placeRows()")
        names = sorted(p["name"] for p in places or [])
        check("2b Glacosite via Namens-Match 'Glacosite (Raw)' aufgelöst → 'Glacier Shelf'",
              "Glacier Shelf" in names, str(places))
        check("2c Riccite-Fallback-Ort 'Ric Highlands' gerendert (T4, max. 743)",
              "Ric Highlands" in names, str(places))
        ric = next((p for p in places or [] if p["name"] == "Ric Highlands"), None)
        ric_text = " ".join(ric["mats"]) if ric else ""
        check("2d Qualitäts-Fallback: Warn-Hinweis place_below_quality am Riccite-Chip",
              ric is not None and "may not reach the required quality (max. 743)" in ric_text, ric_text)
        glacier = next((p for p in places or [] if p["name"] == "Glacier Shelf"), None)
        glacier_text = " ".join(glacier["mats"]) if glacier else ""
        check("2e Normal qualifizierender Ort (Glacosite) OHNE Warnung",
              glacier is not None and "may not reach" not in glacier_text, glacier_text)
        check("2f Kein uncovered-Rest (beide Materials covered)",
              (await js("uncovered()")) == [], str(await js("uncovered()")))
        ores_calls = await js("callsOf('strata_ores')") or []
        check("2g strata_ores für den Fallback gerufen", len(ores_calls) >= 1, str(len(ores_calls)))
        await shot("m9o-places.png")

        # ---- Gate 3: Deutsch — Warn-Text live (i18n-Parität) ----
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": "window.__M9O_LANG = 'de';"})
        await js("location.reload()")
        await wait_for("document.querySelector('.rail .rentry')", 15)
        n2 = await wait_for("window.setupList()", 25)
        check("3a DE-Setup: 2 Einträge", n2 == 2, str(n2))
        await js("computePlaces()")
        await wait_for("placeRows().length > 0", 12)
        places_de = await js("placeRows()")
        ric_de = next((p for p in places_de or [] if p["name"] == "Ric Highlands"), None)
        ric_de_text = " ".join(ric_de["mats"]) if ric_de else ""
        check("3b DE: 'Ort erreicht ggf. nicht die geforderte Qualität (max. 743)'",
              "geforderte Qualität (max. 743)" in ric_de_text, ric_de_text)
        await shot("m9o-de.png")

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
