#!/usr/bin/env python3
"""M9-N probe (t_928d5896) — „Liste abschließen“ konsumiert Reservierungen.

Gates:
  1. Header-Button „Complete list“ über der Liste vorhanden; Klick ruft
     `list_complete` mit der Listen-Id auf.
  2. Konsum (Stub spiegelt die Backend-Logik): reservierte Mengen werden
     von den Inventar-Positionen abgezogen, Listen-Reservierungen gelöst
     (Freizweck bleibt), done = volle Bedarfsmenge (Aggregat zeigt x/x).
  3. Bestätigungs-Note erscheint.

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Ports 8899/9249 (Probe-Hotspot: pro Task eigene Ports).
"""
import asyncio, json, os, pathlib, shutil, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_928d5896")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8899/"

PASS = 0
FAIL = 0

shutil.rmtree("/tmp/cdp-m9n-complete", ignore_errors=True)

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
  // ---- Inventar: Hadanite 300 (davon 300 für Liste 1 reserviert) +
  //      Freizweck-Reservierung (20, 'Verkauf') + Quantum Core 10 (2 reserviert) ----
  const INV = [
    { id: '1', entity_kind: 'commodity', entity_id: '1000', name: 'Hadanite', quality: 500,
      location_id: null, location_name: 'Levski', quantity: 300, price: null,
      reservations: [], created_at: 0, updated_at: 0 },
    { id: '2', entity_kind: 'commodity', entity_id: '1002', name: 'Quantum Core', quality: 500,
      location_id: null, location_name: 'Levski', quantity: 10, price: null,
      reservations: [], created_at: 0, updated_at: 0 },
    { id: '3', entity_kind: 'commodity', entity_id: '1003', name: 'Gold', quality: 500,
      location_id: null, location_name: 'Heim-Hangar', quantity: 50, price: null,
      reservations: [{ id: '9003', qty: 20, purpose: 'Verkauf', list_id: null, list_name: null, note: '' }],
      created_at: 0, updated_at: 0 }
  ];

  const ITEMS = { items: [
    { id: 'item-0', name: 'Plain Cannon', slug: 'plain-cannon', category: 'weapon' }
  ], count: 1, attribution: 'Data from celd.space / Strata Mining Tools' };

  // ---- Listen-Engine (Muster m9-f) ----
  const DB = { seq: 1, lists: [], items: [], mats: [], done: [] };
  window.__LIST = DB;
  window.__CALLS = [];
  window.__INV = INV;
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    window.__CALLS.push({ cmd, args: a });
    if (cmd === 'strata_items') return ITEMS;
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4, scwiki_ready: true };
    if (cmd === 'inventory_list') return INV.map((r) => ({ ...r, reservations: r.reservations.map((x) => ({ ...x })) }));
    if (cmd === 'list_create') {
      const id = DB.seq++;
      DB.lists.push({ id, kind: a.kind, name: a.name, created_at: Math.floor(Date.now() / 1000) });
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
    if (cmd === 'lists_list') return DB.lists.filter((l) => l.kind === a.kind).map((l) => ({
      id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
      item_count: DB.items.filter((i) => i.list_id === l.id).length }));
    if (cmd === 'list_detail') {
      const l = DB.lists.find((x) => x.id === a.listId);
      if (!l) throw new Error('lists.err_missing');
      const map = new Map();
      for (const it of DB.items.filter((i) => i.list_id === l.id)) {
        it.mats = DB.mats.filter((mm) => mm.item_row === it.id);
        for (const m of it.mats) {
          const k = m.material + ':' + m.unit;
          const e = map.get(k) || { material: m.material, unit: m.unit, quantity: 0, min_quality: 0, done_qty: 0,
                                    reserved_qty: 0, reserved_avg_quality: null };
          e.quantity += m.quantity * it.copies;
          e.min_quality = Math.max(e.min_quality, m.quality);
          map.set(k, e);
        }
      }
      for (const d of DB.done.filter((d) => d.list_id === l.id)) {
        const k = d.material + ':' + d.unit;
        if (map.has(k)) map.get(k).done_qty = d.done_qty;
      }
      for (const r of INV) for (const res of r.reservations) {
        if (String(res.list_id) === String(l.id)) {
          const e = [...map.values()].find((m) => m.material.toLowerCase() === r.name.toLowerCase());
          if (e) e.reserved_qty += res.qty;
        }
      }
      return { id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
        items: DB.items.filter((i) => i.list_id === l.id).map((i) => ({ ...i, mats: DB.mats.filter((mm) => mm.item_row === i.id) })),
        aggregated: [...map.values()].sort((a, b) => a.material.localeCompare(b.material)) };
    }
    // M9-N: „Liste abschließen“ — Spiegel der Backend-Logik: reservierte
    // Mengen von den Positionen abziehen (defensiv auf 0 geklemmt),
    // Listen-Reservierungen löschen (Freizweck bleibt), done = Soll.
    if (cmd === 'list_complete') {
      const l = DB.lists.find((x) => x.id === a.listId);
      if (!l) throw new Error('lists.err_missing');
      const reqs = new Map();
      for (const it of DB.items.filter((i) => i.list_id === l.id)) {
        for (const m of DB.mats.filter((mm) => mm.item_row === it.id)) {
          const k = m.material + ':' + m.unit;
          reqs.set(k, (reqs.get(k) || 0) + m.quantity * it.copies);
        }
      }
      for (const r of INV) {
        r.reservations = r.reservations.filter((x) => {
          if (String(x.list_id) === String(l.id)) {
            r.quantity = Math.max(r.quantity - x.qty, 0);
            return false;
          }
          return true;
        });
      }
      for (const [k, q] of reqs) {
        const [material, unit] = [k.slice(0, k.lastIndexOf(':')), k.slice(k.lastIndexOf(':') + 1)];
        DB.done.push({ list_id: l.id, material, unit, done_qty: q });
      }
      return null;
    }
    return orig(cmd, args);
  };

  // ---- Probe-Helfer ----
  window.pressKey = (key) => window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  const qa = (s) => [...document.querySelectorAll(s)];
  window.callsOf = (cmd) => window.__CALLS.filter((c) => c.cmd === cmd);
  window.craftLists = () => !!document.querySelector('.clists');
  window.aggTexts = () => qa('.clists .cl-agg-row').map((e) => e.textContent.replace(/\s+/g, ' ').trim());
  window.resvChips = () => qa('.clists .cl-resv').map((e) => e.textContent.trim());
  window.noteText = () => { const n = document.querySelector('.cl-inv-note'); return n ? n.textContent.trim() : null; };
  window.clickCompleteList = () => {
    const b = [...qa('.clists .cl-head .cl-mini')].find((x) =>
      x.textContent.trim() === 'Complete list' || x.textContent.trim() === 'Liste abschließen');
    b?.click();
    return !!b;
  };
  window.setListName = (v) => {
    const i = document.querySelector('.clists .cl-input');
    if (!i) return false;
    i.value = v; i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.clickNewList = () => { const b = [...qa('.clists .cl-btn')][0]; b?.click(); return !!b; };
  // M9-K P11: Dropdown (ListPicker) statt Chips — Craft-Variante (.clists).
  window.clLpLabel = () => document.querySelector('.clists .lp-btn .lp-label')?.textContent.trim() || '';
  window.clLpPopOpen = () => !!document.querySelector('.clists .lp-pop');
  window.clLpRows = () => qa('.clists .lp-pop .lp-row .lp-row-name').map((x) => x.textContent.trim());
  window.clLpRect = () => { const b = document.querySelector('.clists .lp-btn'); if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; };
  window.clLpRowRect = (name) => {
    const b = qa('.clists .lp-pop .lp-row').find((x) => x.querySelector('.lp-row-name')?.textContent.trim() === name);
    if (!b) return null; const r = b.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  };
  window.clickChip = (idx) => { const c = qa('.clists .cl-chip')[idx]; c?.click(); return !!c; };
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
        "--remote-debugging-port=9249", "--user-data-dir=/tmp/cdp-m9n-complete",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9249/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        server.terminate(); proc.terminate()
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

        async def mouse_click(pt):
            """ECHTER Maus-Klick über CDP-Input — der Dropdown-Regressionfall."""
            if not pt:
                return False
            for t in ("mousePressed", "mouseReleased"):
                ok(await call("Input.dispatchMouseEvent", {
                    "type": t, "x": pt["x"], "y": pt["y"],
                    "button": "left", "clickCount": 1}))
            return True

        async def shot(name):
            data = ok(await call("Page.captureScreenshot", {"format": "png"}))["data"]
            (OUT / name).write_bytes(__import__("base64").b64decode(data))
            print(f"  shot {OUT / name}")

        await wrap(WRAP)
        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        # Crafting-Listen-Ansicht öffnen
        await js("window.pressKey('F6')")
        if not await wait_for("document.querySelector('.crafting')", 6):
            await asyncio.sleep(1.0)
            await js("window.pressKey('F6')")
            await wait_for("document.querySelector('.crafting')", 8)
        await js("""
          (() => { const b = [...document.querySelectorAll('.crafting .ctab')]
            .find((x) => x.textContent.trim() === 'Lists'); b?.click(); return !!b; })()
        """)
        await wait_for("craftLists()", 8)
        await wait_for("craftLists()", 8)
        check("0a Liste über die UI angelegt", await js("setListName('Rasierer-Set')") is True)
        await asyncio.sleep(0.2)
        await js("clickNewList()")
        await wait_for("document.querySelectorAll('.clists .cl-chip').length === 1", 8)
        # Eintrag: Hadanite 10 SCU (Bedarf) + Quantum Core 2 items; Reser-
        # vierungen im Stub VOR dem ersten Chip-Klick vorkehren (M9-I-
        # Klemmung: vorpflanzte Reservierungen erst NACH dem Anlegen).
        await js("""
          (async () => {
            const l = window.__LIST.lists[0].id;
            await window.__TAURI_INTERNALS__.invoke('list_add_item', { listId: l,
              item: { item_id: 'bp1', item_name: 'Rifle', craft_time: 60, copies: 1,
                      mats: [ { material: 'Hadanite', ore_id: null, unit: 'SCU', quantity: 10, quality: 500 },
                              { material: 'Quantum Core', ore_id: null, unit: 'items', quantity: 2, quality: 500 } ] } });
            const id = String(l);
            window.__INV[0].reservations.push({ id: '9101', qty: 10, purpose: '', list_id: id, list_name: 'Rasierer-Set', note: 'Hadanite' });
            window.__INV[1].reservations.push({ id: '9102', qty: 2, purpose: '', list_id: id, list_name: 'Rasierer-Set', note: 'Quantum Core' });
            return true;
          })()
        """)
        # Liste ist nach dem Anlegen auto-aktiv — Detail neu laden über
        # den ListPicker (M9-K P11: Dropdown statt Chips): Pop öffnen,
        # Zeile 'Rasierer-Set' per ECHTEM Maus-Klick wählen.
        await mouse_click(await js("clLpRect()"))
        await wait_for("clLpPopOpen()", 5)
        check("0b1 Dropdown listet die Liste", "Rasierer-Set" in (await js("clLpRows()")), str(await js("clLpRows()")))
        await mouse_click(await js("clLpRowRect('Rasierer-Set')"))
        await wait_for("!clLpPopOpen()", 5)
        await wait_for("aggTexts().length === 2", 8)
        await asyncio.sleep(0.4)
        aggs = await js("aggTexts()")
        check("0b Vorher reserviert: Hadanite-Chip 10 sichtbar",
              any("reserved 10" in a for a in aggs)
              or any("reserved 10" in c for c in (await js("resvChips()"))), str(aggs))
        await shot("m9n-before-complete.png")

        # ---- Gate 1: Button über der Liste + Klick ruft Command auf ----
        check("1a „Complete list“-Button im Listen-Header vorhanden", await js("clickCompleteList()") is True)
        await wait_for("callsOf('list_complete').length === 1", 8)
        call1 = (await js("callsOf('list_complete')"))[0]
        check("1b Command mit Listen-Id", str(call1["args"]["listId"]) == "1", str(call1["args"]))

        # ---- Gate 2: Konsum + done = Soll ----
        await asyncio.sleep(0.4)
        inv = await js("window.__INV.map((r) => r.name + '=' + r.quantity + ' res:' + r.reservations.map((x) => (x.list_id ?? 'free') + ':' + x.qty).join(',')).join(' | ')")
        check("2a Hadanite konsumiert (300→290, Listen-Reservierung weg)",
              "Hadanite=290" in inv and "Hadanite=290 res:" in inv, inv)
        check("2b Quantum Core konsumiert (10→8)", "Quantum Core=8" in inv, inv)
        check("2c Freizweck-Reservierung bleibt (free:20)", "free:20" in inv, inv)
        aggs = await js("aggTexts()")
        check("2d done = Soll (Hadanite 10/10, Quantum Core 2/2)",
              aggs and any("10/10" in a for a in aggs) and any("2/2" in a for a in aggs), str(aggs))
        chips = await js("resvChips()")
        check("2e keine Listen-Reservierungs-Chips mehr", len(chips) == 0, str(chips))

        # ---- Gate 3: Bestätigungs-Note ----
        note = await js("noteText()")
        check("3a Note erscheint", note is not None and ("completed" in note or "abgeschlossen" in note), str(note))
        await shot("m9n-after-complete.png")

    server.terminate()
    proc.terminate()
    print(f"\nRESULT: PASS={PASS} FAIL={FAIL}")
    raise SystemExit(0 if FAIL == 0 else 1)


asyncio.run(main())
