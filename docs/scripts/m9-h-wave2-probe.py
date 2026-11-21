#!/usr/bin/env python3
"""M9-H probe (t_7709c7f2) — Welle 2 UX-Fixes.

Gates:
  1. Tab-Rückweg: die Mining-Tab-Zeile (Ores/Locations/Lists) bleibt im
     Listen-Reiter sichtbar — Klick auf „Ores“ führt zurück (Konsistenz
     mit Crafting .ctabs).
  2. Scroll+Padding: .mlists und .clists sind eigene Scroll-Container
     (overflow-y auto) mit horizontalem Padding; Blueprint-Details
     (.cdetail) haben horizontales Padding.
  3. Autocomplete-Doppel: im Inventar-Formular gibt es KEIN natives
     <datalist> mehr — gespeicherte Depot-Orte (·) und SC-Location-
     Entities (l) liegen in EINER Custom-Liste (.if-picks).
  4. Mining-Liste → Inventar mit Vorbefüllung: „Done“ öffnet die Maske
     mit Restbetrag voreingetragen (nur Qualität+Lagerort); Submit
     erzeugt Inventar-Position + list_set_done. Standalone-Liste →
     inventory_add (KEINE Reservierung); verknüpfte Liste (source_list_id)
     → inventory_add_for_list (Reservierung).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Ports 8899/9247 (Probe-Hotspot: pro Task eigene Ports).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_7709c7f2")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8899/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m9h-wave2", ignore_errors=True)

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
  // Strata-Referenz: 1 Erz (ship→SCU) reicht für die Maske.
  const ORES = { ores: [
    { id: 'ore-hadanite', name: 'Hadanite', slug: 'hadanite', category: 'ship', tier: 'uncommon',
      scanSignature: 12, instability: 0.3, resistance: 0.4 }
  ], count: 1, attribution: 'Data from celd.space / Strata Mining Tools' };

  // ---- In-Memory Listen-Engine (kind-aware, M9-C-Muster + source_list_id) ----
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
    // Crafting-Modus (F6) lädt Items — leere Antwort genügt für die Listen-Tab-Geometrie.
    if (cmd === 'strata_items') return { items: [], count: 0, attribution: 'Data from celd.space / Strata Mining Tools' };
    if (cmd === 'strata_locations') return { locations: [], count: 0, attribution: 'x' };
    if (cmd === 'lists_list') return DB.lists.filter((l) => l.kind === a.kind).map((l) => ({
      id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
      item_count: DB.items.filter((i) => i.list_id === l.id).length,
      source_list_id: l.source_list_id ?? null,
      source_list_name: l.source_list_id != null
        ? (DB.lists.find((s) => s.id === l.source_list_id) || {}).name ?? ('#' + l.source_list_id)
        : null }));
    if (cmd === 'list_create') {
      const id = DB.seq++;
      DB.lists.push({ id, kind: a.kind, name: a.name, created_at: Math.floor(Date.now() / 1000),
                      source_list_id: a.sourceListId ?? null });
      return id;
    }
    if (cmd === 'list_add_item') {
      const it = a.item;
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
    // M9-H Punkt 5: Reservierung NUR bei verknüpften Listen wird über diesen
    // Command laufen — standalone geht inventory_add.
    if (cmd === 'inventory_add_for_list') {
      const row = await orig('inventory_add', { entry: a.entry });
      DB.__resv = DB.__resv || [];
      DB.__resv.push({ list_id: a.listId, material: a.material, unit: a.unit, inv: row.id });
      return { entry: row, requirement_raised: false, requirement: a.entry.quality };
    }
    return orig(cmd, args);
  };

  // ---- Probe-Helfer ----
  window.pressKey = (key) => window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  window.mtabsVisible = () => !!document.querySelector('.mining > .mtabs');
  window.mtabsCount = () => document.querySelectorAll('.mining .mtab').length;
  const q = (s) => document.querySelector(s);
  window.listRegion = () => !!q('.mining .mlists');
  window.styleOf = (sel) => {
    const el = q(sel);
    if (!el) return null;
    const s = getComputedStyle(el);
    return { overflowY: s.overflowY, padL: parseFloat(s.paddingLeft), padR: parseFloat(s.paddingRight),
             padB: parseFloat(s.paddingBottom), sh: el.scrollHeight, ch: el.clientHeight };
  };
  window.clickMiningTab = (label) => {
    const b = [...document.querySelectorAll('.mining .mtab')].find((x) => x.textContent.trim() === label);
    if (!b) return false;
    b.click();
    return true;
  };
  // Listen-Tab-Setup: Liste anlegen + Material 250 hinzufügen.
  window.setupList = async () => {
    const i = q('.mining .ml-new .ml-input');
    i.value = 'Smoke-Liste'; i.dispatchEvent(new Event('input', { bubbles: true }));
    [...document.querySelectorAll('.mining .ml-btn')].find((x) => !x.disabled)?.click();
    await new Promise((r) => setTimeout(r, 300));
    const inputs = [...document.querySelectorAll('.mining .ml-add .ml-input')];
    inputs[0].value = 'hadanite'; inputs[0].dispatchEvent(new Event('input', { bubbles: true }));
    await new Promise((r) => setTimeout(r, 200));
    [...document.querySelectorAll('.mining .ml-suggest-row')].find((x) => x.textContent.includes('Hadanite'))?.click();
    await new Promise((r) => setTimeout(r, 200));
    inputs[1].value = '250'; inputs[1].dispatchEvent(new Event('input', { bubbles: true }));
    inputs[2].value = '800'; inputs[2].dispatchEvent(new Event('input', { bubbles: true }));
    [...document.querySelectorAll('.mining .ml-add .ml-btn')].find((x) => !x.disabled)?.click();
    await new Promise((r) => setTimeout(r, 400));
    return document.querySelectorAll('.mining .ml-agg-row').length;
  };
  window.openInvForm = (idx) => {
    const rows = document.querySelectorAll('.mining .ml-agg-row');
    const btns = [...(rows[idx]?.querySelectorAll('.ml-agg-actions .ml-mini') || [])];
    const b = btns.find((x) => x.textContent.trim().toLowerCase().startsWith('done'));
    if (!b) return false;
    b.click();
    return true;
  };
  window.invForm = () => {
    const f = q('.mining .ml-inv-form');
    if (!f) return null;
    const ins = [...f.querySelectorAll('input')];
    return { loc: ins[0]?.value, q: ins[1]?.value, qty: ins[2]?.value };
  };
  window.fillInvForm = (loc) => {
    const f = q('.mining .ml-inv-form');
    if (!f) return false;
    const i = f.querySelector('input');
    i.value = loc; i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.clickInvAdd = () => {
    const f = q('.mining .ml-inv-form');
    if (!f) return false;
    [...f.querySelectorAll('button')].find((b) => b.classList.contains('ml-btn'))?.click();
    return true;
  };
  window.invNoteText = () => q('.mining .ml-inv-note')?.textContent.trim() ?? null;
  window.aggText = (idx) => document.querySelectorAll('.mining .ml-agg-row')[idx]?.textContent ?? '';
  window.makeLinked = async () => {
    // Verknüpfte Liste direkt über die Stub-Grenze anlegen (Brücken-Snapshot:
    // source_list_id = erzeugende Crafting-Liste). Das UI erfährt davon erst
    // über den nächsten loadLists-RUNDEN — ausgelöst durch eine UI-Anlage.
    await window.__TAURI_INTERNALS__.invoke('list_create', { kind: 'mining', name: 'Linked', sourceListId: 7 });
    const i = q('.mining .ml-new .ml-input');
    i.value = 'Refresh'; i.dispatchEvent(new Event('input', { bubbles: true }));
    [...document.querySelectorAll('.mining .ml-btn')].find((x) => !x.disabled)?.click();
    await new Promise((r) => setTimeout(r, 500));
    const chip = [...document.querySelectorAll('.mining .ml-chip')].find((c) => c.textContent.includes('Linked'));
    if (!chip) return false;
    chip.click();
    await new Promise((r) => setTimeout(r, 500));
    return true;
  };
  window.addMatToList = async (name) => {
    const inputs = [...document.querySelectorAll('.mining .ml-add .ml-input')];
    inputs[0].value = name; inputs[0].dispatchEvent(new Event('input', { bubbles: true }));
    await new Promise((r) => setTimeout(r, 200));
    [...document.querySelectorAll('.mining .ml-suggest-row')].find((x) => x.textContent.includes('Hadanite'))?.click();
    await new Promise((r) => setTimeout(r, 200));
    inputs[1].value = '100'; inputs[1].dispatchEvent(new Event('input', { bubbles: true }));
    [...document.querySelectorAll('.mining .ml-add .ml-btn')].find((x) => !x.disabled)?.click();
    await new Promise((r) => setTimeout(r, 400));
    return document.querySelectorAll('.mining .ml-agg-row').length;
  };
  // Inventar: Formular öffnen + Standort eintippen (Autocomplete-Doppel).
  window.openInvAdd = async () => {
    window.pressKey('F4');
    await new Promise((r) => setTimeout(r, 400));
    document.querySelector('.inv-bar .chip.add')?.click();
    await new Promise((r) => setTimeout(r, 300));
    return !!q('.inv-form');
  };
  window.typeLoc = (txt) => {
    const sects = [...document.querySelectorAll('.inv-form .if-sect')];
    const locSect = sects.find((s) => s.querySelector('input[list]') || [...s.querySelectorAll('.h')].some((h) => h.textContent.toLowerCase().includes('location')));
    const inp = locSect?.querySelector('input.if-input');
    if (!inp) return false;
    inp.value = txt; inp.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.locPicks = () => [...document.querySelectorAll('.inv-form .if-pick')].map((b) => ({
    k: b.querySelector('.k')?.textContent.trim(), text: b.textContent.trim().replace(/\s+/g, ' ') }));
  window.hasDatalist = () => !!q('.inv-form datalist');
  window.callsOf = (cmd) => window.__CALLS.filter((c) => c.cmd === cmd);
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
        "--remote-debugging-port=9247", "--user-data-dir=/tmp/cdp-m9h-wave2",
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

        # ---- Gate 1: Tab-Rückweg im Listen-Reiter ----
        await js("window.pressKey('F5')")
        await wait_for("document.querySelector('.mining')", 8)
        await wait_for("mtabsCount() === 3", 8)
        check("1a Tab-Zeile im Ores-View sichtbar", await js("mtabsVisible()") is True)
        check("1b Tab-Klick öffnet Listen-View", await js("clickMiningTab('Lists')") is True)
        await wait_for("listRegion()", 8)
        check("1c MiningLists gerendert", await js("listRegion()") is True)
        check("1d Rückweg: Tab-Zeile IMMER im Listen-View (Fix Punkt 1)",
              await js("mtabsVisible()") is True and await js("mtabsCount()") == 3,
              str(await js("mtabsCount()")))
        check("1e Rückweg-Klick zurück zu Ores", await js("clickMiningTab('Ores')") is True)
        await wait_for("!listRegion()", 8)
        check("1f Ores-View wieder aktiv", await js("listRegion()") is False)
        await js("clickMiningTab('Lists')")
        await wait_for("listRegion()", 8)

        # ---- Gate 2: Scroll+Padding (beide Listen + Detail) ----
        ml = await js("styleOf('.mining .mlists')")
        check("2a .mlists overflow-y auto", ml and ml["overflowY"] == "auto", str(ml))
        check("2b .mlists horizontales Padding", ml and ml["padR"] >= 8 and ml["padL"] >= 2, str(ml))
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("document.activeElement && document.activeElement.blur(); true;")
        await js("""
          var tabs=[...document.querySelectorAll('.crafting .ctab')].map(function(x){return x.textContent.trim();});
          window.__ctabsDebug = tabs;
          var b=tabs.indexOf('Lists')>=0
            ? [...document.querySelectorAll('.crafting .ctab')][tabs.indexOf('Lists')]
            : document.querySelectorAll('.crafting .ctab')[1];
          if(b) b.click(); true;""")
        await asyncio.sleep(0.5)
        print("  debug ctabs:", await js("window.__ctabsDebug"),
              "on:", await js("document.querySelector('.crafting .ctab.is-on')?.textContent || null"),
              "clists:", await js("!!document.querySelector('.crafting .clists')"),
              "err:", await js("window.__pageErr || null"))
        await wait_for("document.querySelector('.crafting .clists')", 8)
        cl = await js("styleOf('.crafting .clists')")
        check("2c .clists overflow-y auto", cl and cl["overflowY"] == "auto", str(cl))
        check("2d .clists horizontales Padding", cl and cl["padR"] >= 8 and cl["padL"] >= 2, str(cl))
        await shot("m9h-craft-lists.png")

        # ---- Gate 3: Autocomplete-Doppel im Inventar ----
        check("3a Inventar-Formular öffnet", await js("openInvAdd()") is True)
        await wait_for("document.querySelector('.inv-form')", 8)
        check("3b KEIN natives <datalist> mehr (Fix Punkt 4)", await js("hasDatalist()") is False)
        check("3c Standort-Feld tippbar", await js("typeLoc('c')") is True)
        await wait_for("(locPicks() || []).some(function(p){return p.k==='l';})", 8)
        picks = await js("locPicks()")
        saved = [p for p in picks or [] if p["k"] == "·"]
        ents = [p for p in picks or [] if p["k"] == "l"]
        check("3d Gespeicherte Depot-Orte (·) in derselben Liste",
              any("Meine Carrack" in p["text"] for p in saved), str(picks))
        check("3e SC-Location-Entities (l) in derselben Liste",
              len(ents) > 0, str(picks))
        await shot("m9h-inv-loc-picks.png")
        await js("window.pressKey('Escape')")  # Formular zu
        await asyncio.sleep(0.3)
        await js("window.pressKey('Escape')")  # Inventar zurück

        # ---- Gate 4: Mining-Liste → Inventar mit Vorbefüllung ----
        await js("window.pressKey('F5')")
        await wait_for("mtabsCount() === 3", 8)
        await js("clickMiningTab('Lists')")
        await wait_for("listRegion()", 8)
        n = await js("setupList()")
        check("4a Aggregations-Zeile vorhanden (0/250)", n == 1 and "0/250" in (await js("aggText(0)")),
              str(await js("aggText(0)")))
        check("4b 'Done' öffnet Inventar-Maske", await js("openInvForm(0)") is True)
        await wait_for("invForm() !== null", 8)
        form = await js("invForm()")
        check("4c Restbetrag voreingetragen (qty=250)", form and form["qty"] == "250", str(form))
        check("4d Qualität voreingetragen (Anforderung 800)", form and form["q"] == "800", str(form))
        check("4e Lagerort leer (nur Q+Ort abfragen)", form and form["loc"] == "", str(form))
        await js("fillInvForm('Depot Smoke')")
        await asyncio.sleep(0.2)
        await js("clickInvAdd()")
        await wait_for("invNoteText() !== null", 8)
        note = await js("invNoteText()") or ""
        check("4f Erfolgshinweis 'Position recorded' (standalone, KEIN 'reserviert')",
              "Position recorded" in note and "reserviert" not in note, str(note))
        await asyncio.sleep(0.3)
        agg = await js("aggText(0)")
        check("4g Listen-Fortschritt mitgeführt (250/250)", "250/250" in agg, str(agg))
        inv_adds = await js("callsOf('inventory_add')")
        inv_for_list = await js("callsOf('inventory_add_for_list')")
        check("4h Standalone-Liste → inventory_add (keine Reservierung)",
              len(inv_adds) >= 1 and len(inv_for_list) == 0,
              f"add={len(inv_adds)} for_list={len(inv_for_list)}")
        dones = await js("callsOf('list_set_done')")
        check("4i list_set_done 250 durch die Maske",
              any((d["args"].get("doneQty") == 250) for d in dones), str(dones))
        await shot("m9h-inv-mask-standalone.png")

        # Verknüpfte Liste: Reservierung via inventory_add_for_list
        check("4j Verknüpfte Liste öffnet (Brücken-Kind)", await js("makeLinked()") is True)
        await js("addMatToList('hadanite')")
        await wait_for("document.querySelectorAll('.mining .ml-agg-row').length >= 1", 8)
        src_badge = await js("document.querySelector('.mining .ml-src')?.textContent || ''")
        check("4k Quelle-Badge sichtbar (verknüpft)", "Craft" in src_badge or "aus" in src_badge or bool(src_badge), str(src_badge))
        await js("openInvForm(0)")
        await wait_for("invForm() !== null", 8)
        await js("fillInvForm('Depot Linked')")
        await asyncio.sleep(0.2)
        await js("clickInvAdd()")
        await wait_for("invNoteText() !== null", 8)
        note2 = await js("invNoteText()") or ""
        check("4l Verknüpfte Liste → Hinweis 'reserviert für'", "reserved for" in note2, str(note2))
        inv_for_list = await js("callsOf('inventory_add_for_list')") or []
        check("4m inventory_add_for_list mit listId gerufen",
              len(inv_for_list) >= 1 and inv_for_list[-1]["args"].get("listId") is not None, str(inv_for_list))
        await shot("m9h-inv-mask-linked.png")

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
