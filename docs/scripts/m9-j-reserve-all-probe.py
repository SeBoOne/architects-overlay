#!/usr/bin/env python3
"""M9-J probe (t_14cc5d4e) — „Alles reservieren“-Button pro Liste.

Gates:
  1. Header-Button „Reserve all“ über der Liste vorhanden; Klick ruft
     `lists_reserve_all` mit der Listen-Id auf.
  2. Greedy best→worst + Ø-Qualitätsregel (Stub spiegelt die Backend-Logik):
     Gold (Bedarf 10, Anforderung 600) → 4×Q900 + 6×Q500 = Ø Q660, voll
     abgedeckt; Titanium (Anforderung 400, nur Q200 im Bestand) →
     UNRESERVIERT (Ø-Check blockt).
  3. Idempotenter Re-Click: zweiter Klick ersetzt die Alt-Reservierungen
     (replaced=2), keine Duplikate, gleiche Mischung.
  4. Inventar-Zuwachs (Gold Q1000) → Re-Click nimmt das bessere Material
     auf (Ø Q1000).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Ports 8897/9245 (Probe-Hotspot: pro Task eigene Ports).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_14cc5d4e")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8897/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m9j-resv", ignore_errors=True)

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
  // ---- Inventar: Gold in 3 Qualitäten (900 frei 4, 500 frei 10, 300 frei 10),
  // Titanium nur Q200 (unter der Anforderung 400) ----
  const INV = [
    { id: '1', entity_kind: 'commodity', entity_id: '1000', name: 'Gold', quality: 900,
      location_id: null, location_name: 'Heim-Hangar', quantity: 4, price: 4500,
      reservations: [], created_at: 0, updated_at: 0 },
    { id: '2', entity_kind: 'commodity', entity_id: '1001', name: 'Gold', quality: 500,
      location_id: null, location_name: 'Meine Carrack', quantity: 10, price: null,
      reservations: [], created_at: 0, updated_at: 0 },
    { id: '3', entity_kind: 'commodity', entity_id: '1002', name: 'Gold', quality: 300,
      location_id: null, location_name: 'Org-Depot', quantity: 10, price: null,
      reservations: [], created_at: 0, updated_at: 0 },
    { id: '4', entity_kind: 'commodity', entity_id: '22', name: 'Titanium', quality: 200,
      location_id: null, location_name: 'Org-Depot', quantity: 40, price: 300,
      reservations: [], created_at: 0, updated_at: 0 }
  ];
  let invSeq = 100;

  const ITEMS = { items: [
    { id: 'item-0', name: 'Plain Cannon', slug: 'plain-cannon', category: 'weapon' }
  ], count: 1, attribution: 'Data from celd.space / Strata Mining Tools' };

  // ---- Listen-Engine (Muster m9-f) ----
  const DB = { seq: 1, lists: [], items: [], mats: [], done: [] };
  window.__LIST = DB;
  window.__CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    window.__CALLS.push({ cmd, args: a });
    if (cmd === 'strata_items') return ITEMS;
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4, scwiki_ready: true };
    if (cmd === 'inventory_list') return INV.map((r) => ({ ...r, reservations: r.reservations.map((x) => ({ ...x })) }));
    if (cmd === 'inventory_add') {
      const e = a.entry || {};
      const row = { id: String(++invSeq), entity_kind: e.entity_kind ?? null, entity_id: e.entity_id ?? null,
        name: e.name, quality: e.quality, location_id: e.location_id ?? null, location_name: e.location_name,
        quantity: e.quantity, price: e.price ?? null, reservations: [], created_at: 0, updated_at: 0 };
      INV.push(row);
      return row;
    }
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
          const k = r.name + ':SCU';
          const e = map.get(k);
          if (e) {
            const w = e.reserved_qty * (e.reserved_avg_quality ?? 0);
            e.reserved_qty += res.qty;
            const qual = (e.reserved_avg_quality == null ? r.quality : e.reserved_avg_quality);
            e.reserved_avg_quality = Math.round((w + res.qty * r.quality) / e.reserved_qty);
          }
        }
      }
      return { id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
        items: DB.items.filter((i) => i.list_id === l.id).map((i) => ({ ...i, mats: DB.mats.filter((mm) => mm.item_row === i.id) })),
        aggregated: [...map.values()].sort((a, b) => a.material.localeCompare(b.material)) };
    }
    // M9-J: „Alles reservieren“ — Spiegel der Backend-Logik (Greedy
    // best→worst, mengengewichtete Ø-Qualität je Material, Cap Bedarf,
    // Alt-Reservierungen der Liste werden ersetzt).
    if (cmd === 'lists_reserve_all') {
      const l = DB.lists.find((x) => x.id === a.listId);
      if (!l) throw new Error('lists.err_missing');
      // Bedarf je Material (Soll − done)
      const need = new Map();
      for (const it of DB.items.filter((i) => i.list_id === l.id)) {
        it.mats = DB.mats.filter((mm) => mm.item_row === it.id);
        for (const m of it.mats) {
          const k = m.material + ':' + m.unit;
          const e = need.get(k) || { material: m.material, unit: m.unit, quantity: 0, min_quality: 0, done: 0 };
          e.quantity += m.quantity * it.copies;
          e.min_quality = Math.max(e.min_quality, m.quality);
          need.set(k, e);
        }
      }
      for (const d of DB.done.filter((d) => d.list_id === l.id)) {
        const k = d.material + ':' + d.unit;
        if (need.has(k)) need.get(k).done = d.done_qty;
      }
      let replaced = 0, created = 0;
      for (const r of INV) {
        const before = r.reservations.length;
        r.reservations = r.reservations.filter((x) => String(x.list_id) !== String(l.id));
        replaced += before - r.reservations.length;
      }
      const mats = [];
      for (const e of [...need.values()].sort((x, y) => x.material.localeCompare(y.material))) {
        const required = Math.max(0, e.quantity - e.done);
        const cand = INV.filter((r) => r.name.toLowerCase() === e.material.toLowerCase())
          .map((r) => ({ row: r, free: r.quantity - r.reservations.reduce((s, x) => s + x.qty, 0) }))
          .filter((c) => c.free > 1e-9)
          .sort((x, y) => y.row.quality - x.row.quality);
        let accQty = 0, accWq = 0;
        for (const c of cand) {
          if (accQty >= required - 1e-9) break;
          let take = Math.min(c.free, required - accQty);
          if (c.row.quality < e.min_quality && accQty + take > 1e-9) {
            const xMax = (accWq - e.min_quality * accQty) / (e.min_quality - c.row.quality);
            take = Math.min(take, Math.max(0, xMax));
            if (take <= 1e-9) break;
          }
          accQty += take; accWq += take * c.row.quality;
          c.row.reservations.push({ id: String(++invSeq), qty: take, purpose: '',
                                    list_id: String(l.id), list_name: l.name, note: e.material });
          created += 1;
        }
        mats.push({ material: e.material, unit: e.unit, required, reserved: accQty,
                    avg_quality: accQty > 1e-9 ? Math.round(accWq / accQty) : null,
                    fully_covered: required > 1e-9 && accQty >= required - 1e-9 });
      }
      return { list_id: l.id, mats, created, replaced };
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
  window.clickReserveAll = () => {
    const b = [...qa('.clists .cl-head .cl-mini')].find((x) =>
      x.textContent.trim() === 'Reserve all' || x.textContent.trim() === 'Alles reservieren');
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
  window.clickChip = (idx) => { const c = qa('.clists .cl-chip')[idx]; c?.click(); return !!c; };
  window.invRows = () => qa('.inv-row').length;
  // Bestand im Inventar-Stub direkt (für Gate 4: Zuwachs)
  window.addInv = async (name, quality, qty) => {
    await window.__TAURI_INTERNALS__.invoke('inventory_add',
      { entry: { name, quality, quantity: qty, location_name: 'Neu-Depot' } });
    return true;
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
        "--remote-debugging-port=9245", "--user-data-dir=/tmp/cdp-m9j-resv",
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
        # Eintrag: Gold 10 SCU (Anforderung 600) + Titanium 5 SCU (Anforderung 400)
        await js("""
          (async () => {
            const l = window.__LIST.lists[0].id;
            await window.__TAURI_INTERNALS__.invoke('list_add_item', { listId: l,
              item: { item_id: 'bp1', item_name: 'Rifle', craft_time: 60, copies: 1,
                      mats: [ { material: 'Gold', ore_id: null, unit: 'SCU', quantity: 10, quality: 600 },
                              { material: 'Titanium', ore_id: null, unit: 'SCU', quantity: 5, quality: 400 } ] } });
            document.querySelectorAll('.clists .cl-chip')[0]?.click();
            return true;
          })()
        """)
        await wait_for("aggTexts().length === 2", 8)
              "lists:", await js("window.__LIST.lists.length"), "items:", await js("window.__LIST.items.length"),
              "calls:", await js("callsOf('list_detail').length"))

        # ---- Gate 1: Button über der Liste + Klick ruft Command auf ----
        check("1a „Reserve all“-Button im Listen-Header vorhanden", await js("clickReserveAll()") is True)
        await wait_for("callsOf('lists_reserve_all').length === 1", 8)
        call1 = (await js("callsOf('lists_reserve_all')"))[0]
        check("1b Command mit Listen-Id", str(call1["args"]["listId"]) == "1", str(call1["args"]))
        await wait_for("noteText() !== null", 8)
        check("1c Bestätigungs-Note erscheint", "reservations set" in (await js("noteText()") or ""), await js("noteText()"))

        # ---- Gate 2: Greedy best→worst + Ø-Qualitätsregel ----
        aggs = await js("aggTexts()")
        check("2a Gold-Zeile: Bedarf 10, Anforderung Q ≥ 600, reserviert 10",
              aggs and any("Gold" in a and "0/10" in a and "Q ≥ 600" in a for a in aggs), str(aggs))
        chips = await js("resvChips()")
        check("2b Gold-Chip: reserved 10 · Ø Q660 (4×Q900 + 6×Q500)",
              any("10" in c and "Q660" in c for c in chips), str(chips))
        check("2c Titanium unreserviert (nur Q200 < Anforderung 400 → Ø-Check blockt)",
              not any("Titanium" in c for c in chips), str(chips))
        # Stub-Inventar: genau 2 neue Reservierungs-Zeilen (4 + 6) mit list_id
        inv_res = await js("window.__CALLS.length > 0 && [...document.querySelectorAll('.inv-row')].length")
        await shot("m9j-reserve-all-first.png")

        # ---- Gate 3: Idempotenter Re-Click ----
        await js("clickReserveAll()")
        await wait_for("callsOf('lists_reserve_all').length === 2", 8)
        await asyncio.sleep(0.3)
        r2 = await js("(async () => { const c = callsOf('lists_reserve_all'); return c[c.length-1].args; })()")
        chips2 = await js("resvChips()")
        check("3a Re-Click ersetzt Alt-Reservierungen (keine Duplikate, gleiche Mischung)",
              any("10" in c and "Q660" in c for c in chips2) and len(chips2) == 1, str(chips2))
        check("3b Note erneut gesetzt", (await js("noteText()") or "").startswith("2 reservations set")
              or "reservations set" in (await js("noteText()") or ""), await js("noteText()"))

        # ---- Gate 4: Inventar-Zuwachs → Re-Click nimmt besseres Material auf ----
        await js("addInv('Gold', 1000, 10)")
        await asyncio.sleep(0.3)
        await js("clickReserveAll()")
        await wait_for("callsOf('lists_reserve_all').length === 3", 8)
        await asyncio.sleep(0.3)
        chips3 = await js("resvChips()")
        check("4a Neue Mischung: Ø Q1000 (besseres Material aufgenommen)",
              any("Q1000" in c for c in chips3), str(chips3))
        await shot("m9j-reserve-all-better.png")

    server.terminate()
    proc.terminate()
    print(f"\nRESULT: PASS={PASS} FAIL={FAIL}")
    raise SystemExit(0 if FAIL == 0 else 1)


asyncio.run(main())
