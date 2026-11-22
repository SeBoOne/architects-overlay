#!/usr/bin/env python3
"""M9-F probe (t_bb97da62) — Inventar-first-Reservierung (Welle 2).

Gates:
  1. Inventar: Zeilen zeigen gesamt/reserviert/frei (Mengen-Breakdown) und
     Reservierungs-Badges mit Zweck (Liste oder Freitext).
  2. Direkte Reservierung im Inventar: R → Panel mit Zweck + Menge,
     reservation_add mit Freitext-Zweck; Badge erscheint live; Badge-✕
     entfernt die Reservierung (reservation_delete).
  3. Crafting-Listen: Aggregierte Zeile zeigt „reserviert X · Ø Q“
     (mengengewichtet) aus list_detail.reserved_qty/reserved_avg_quality.
  4. Aus Inventar reservieren: „Aus Inventar reservieren“ öffnet den
     Quellen-Picker (alle Qualitäten/Standorte, freie Mengen), Auswahl
     reserviert aus freiem Bestand (reservation_add mit list_id).
  5. „Inventar öffnen“-Button in der Crafting-Liste → Inventar-Modus mit
     Erfassungsformular (addTick-Impuls).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Ports 8799/9239 (Probe-Hotspot: pro Task eigene Ports).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_bb97da62")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8799/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m9f-resv", ignore_errors=True)

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
  // ---- Inventar: Gold Q900/Q364 (Reservierungen vorgeplant) + Titanium ----
  // reservations spiegeln das v16-Wire-Format (ids als Strings).
  const INV = [
    { id: '1', entity_kind: 'commodity', entity_id: '1000', name: 'Gold', quality: 900,
      location_id: null, location_name: 'Heim-Hangar', quantity: 12, price: 4500,
      reservations: [{ id: '901', qty: 4, purpose: '', list_id: '10', list_name: 'FPS-Set', note: 'Gold' },
                     { id: '902', qty: 2, purpose: 'Verkauf', list_id: null, list_name: null, note: '' }],
      created_at: 0, updated_at: 0 },
    { id: '2', entity_kind: 'commodity', entity_id: '1001', name: 'Gold', quality: 364,
      location_id: null, location_name: 'Meine Carrack', quantity: 3, price: null,
      reservations: [], created_at: 0, updated_at: 0 },
    { id: '3', entity_kind: 'commodity', entity_id: '22', name: 'Titanium', quality: 720,
      location_id: null, location_name: 'Org-Depot', quantity: 40, price: 300,
      reservations: [], created_at: 0, updated_at: 0 }
  ];
  let invSeq = 100;

  // strata_items (Crafting-Hintergrund; smoke-stub stubbt es nicht — Muster m8-r1a)
  const ITEMS = { items: [
    { id: 'item-0', name: 'Plain Cannon', slug: 'plain-cannon', category: 'weapon' },
    { id: 'item-1', name: 'Quality Cannon', slug: 'quality-cannon', category: 'weapon' }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };

  // ---- Listen-Engine (Muster m9-c) + Reservierungen (M9-F) ----
  const DB = { seq: 1, lists: [], items: [], mats: [], done: [], res: [], resSeq: 1 };
  window.__LIST = DB;

  window.__CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    window.__CALLS.push({ cmd, args: a });
    if (cmd === 'strata_items') return ITEMS;
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4, scwiki_ready: true };
    if (cmd === 'inventory_list') return INV.map((r) => ({ ...r, reservations: r.reservations.map((x) => ({ ...x })) }));
    if (cmd === 'inventory_locations') {
      const seen = new Set(); const names = [];
      for (const r of INV) { const k = r.location_name.toLowerCase(); if (!seen.has(k)) { seen.add(k); names.push(r.location_name); } }
      return names;
    }
    if (cmd === 'reservation_add') {
      const n = a.new;
      const inv = INV.find((r) => String(r.id) === String(n.inventory_id));
      if (!inv) throw new Error('Position ' + n.inventory_id + ' existiert nicht');
      const free = inv.quantity - inv.reservations.reduce((s, x) => s + x.qty, 0);
      if (!(n.qty > 0) || n.qty > free + 1e-9) throw new Error('reservations.err_no_free');
      if (!n.purpose.trim() && !n.list_id) throw new Error('reservations.err_purpose');
      const res = { id: String(++invSeq), qty: n.qty, purpose: n.purpose || '',
                    list_id: n.list_id ? String(n.list_id) : null,
                    list_name: null, note: n.note || '' };
      inv.reservations.push(res);
      return res;
    }
    if (cmd === 'reservation_delete') {
      for (const r of INV) {
        const i = r.reservations.findIndex((x) => String(x.id) === String(a.id));
        if (i >= 0) { r.reservations.splice(i, 1); return null; }
      }
      throw new Error('reservations.err_missing');
    }
    if (cmd === 'inventory_add') {
      const e = a.entry || {};
      const now = Math.floor(Date.now() / 1000);
      const row = { id: String(++invSeq), entity_kind: e.entity_kind ?? null, entity_id: e.entity_id ?? null,
        name: e.name, quality: e.quality, location_id: e.location_id ?? null, location_name: e.location_name,
        quantity: e.quantity, price: e.price ?? null, reservations: [], created_at: now, updated_at: now };
      INV.push(row);
      return row;
    }
    if (cmd === 'inventory_add_for_list') {
      const e = a.entry || {};
      const row = { id: String(++invSeq), entity_kind: null, entity_id: null, name: e.name,
        quality: e.quality, location_id: null, location_name: e.location_name, quantity: e.quantity,
        price: null, reservations: [{ id: String(++invSeq), qty: e.quantity, purpose: '',
                                      list_id: String(a.listId), list_name: 'Craft-Sperrliste', note: e.name }],
        created_at: 0, updated_at: 0 };
      INV.push(row);
      return { entry: row, requirement_raised: false, requirement: e.quality };
    }
    if (cmd === 'lists_list') return DB.lists.filter((l) => l.kind === a.kind).map((l) => ({
      id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
      item_count: DB.items.filter((i) => i.list_id === l.id).length }));
    if (cmd === 'list_create') {
      const id = DB.seq++;
      DB.lists.push({ id, kind: a.kind, name: a.name, created_at: Math.floor(Date.now() / 1000) });
      return id;
    }
    if (cmd === 'list_delete') {
      DB.lists = DB.lists.filter((l) => l.id !== a.listId);
      DB.items = DB.items.filter((i) => i.list_id !== a.listId);
      return null;
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
      // M9-F: reservierte Mengen aus dem Inventar (list_id gematcht,
      // mengengewichtete Ø-Qualität über die Inventar-Qualitäten).
      const listIds = [String(l.id)];
      for (const r of INV) for (const res of r.reservations) {
        if (listIds.includes(String(res.list_id))) {
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
    return orig(cmd, args);
  };

  // ---- Probe-Helfer ----
  window.pressKey = (key) => window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  const q = (s) => document.querySelector(s);
  const qa = (s) => [...document.querySelectorAll(s)];
  window.invRows = () => qa('.inv-row').length;
  window.qtySubs = () => qa('.inv-row .qty-sub').map((e) => e.textContent.trim());
  window.resBadges = () => qa('.inv-row button.resv').map((e) => e.textContent.trim());
  window.clickRowR = (idx) => {
    const btns = qa('.inv-row button.inv-edit').filter((b) => b.textContent.trim() === 'R');
    btns[idx]?.click();
    return !!btns[idx];
  };
  window.resPanelOpen = () => !!q('.res-panel');
  window.setResPurpose = (v) => {
    const i = q('.res-panel .res-input');
    if (!i) return false;
    i.value = v; i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.setResQty = (v) => {
    const i = q('.res-panel .res-input.res-qty');
    if (!i) return false;
    i.value = v; i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.resSave = () => { const b = q('.res-panel .res-save'); b?.click(); return !!b; };
  window.badgeX = (idx) => { const b = qa('.inv-row button.resv')[idx]; b?.click(); return !!b; };
  window.callsOf = (cmd) => window.__CALLS.filter((c) => c.cmd === cmd);

  // Crafting-Listen-Helfer
  window.craftLists = () => !!q('.clists');
  // M9-K (t_db9b234f, P11): Chips durch ListPicker-Dropdown ersetzt.
  window.craftChips = () => {
    const label = q('.clists .lp-btn .lp-label')?.textContent.trim();
    return label ? [label] : [];
  };
  window.setListName = (v) => {
    const i = q('.clists .cl-input');
    if (!i) return false;
    i.value = v; i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.clickNewList = () => { const b = [...qa('.clists .cl-btn')][0]; b?.click(); return !!b; };
  window.aggTexts = () => qa('.clists .cl-agg-row').map((e) => e.textContent.replace(/\s+/g, ' ').trim());
  window.resvChips = () => qa('.clists .cl-resv').map((e) => e.textContent.trim());
  window.clickReserveBtn = () => {
    const b = [...qa('.clists .cl-agg-actions .cl-mini')].find((x) => x.textContent.includes('Inventar reservieren') || x.textContent.includes('Reserve from inventory') || x.textContent.includes('reservieren'));
    b?.click();
    return !!b;
  };
  window.resPickRows = () => qa('.clists .cl-res-pick').map((e) => e.textContent.replace(/\s+/g, ' ').trim());
  window.pickRow = (idx) => {
    const b = qa('.clists .cl-res-pick')[idx];
    b?.click();
    return !!b;
  };
  window.setResPickQty = (v) => {
    const inputs = qa('.clists .cl-res-qty');
    const i = inputs[inputs.length - 1]; // Qty-Input der zuletzt gewählten Quelle
    if (!i) return false;
    i.value = v; i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.resPickSubmit = () => {
    const b = [...qa('.clists .cl-inv-form .cl-btn')].find((x) => x.textContent.includes('Reserve'));
    b?.click();
    return !!b;
  };
  window.clickOpenInventory = () => {
    const b = [...qa('.clists .cl-head .cl-mini')].find((x) => x.textContent.includes('Inventar') || x.textContent.includes('inventory'));
    b?.click();
    return !!b;
  };
})();
"""


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8799", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9239", "--user-data-dir=/tmp/cdp-m9f-resv",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9239/json/list", timeout=1) as r:
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

        # ---- Gate 1+2: Inventar — gesamt/reserviert/frei + direkte Reservierung ----
        await js("window.pressKey('F4')")
        await wait_for("invRows() === 3", 8)
        subs = await js("qtySubs()")
        check("1a Zeile 1 zeigt Breakdown 6 reserved / 6 free",
              subs == ["6 reserved · 6 free"], str(subs))
        badges = await js("resBadges()")
        check("1b Reservierungs-Badges (Liste + Freitext-Zweck)",
              len(badges) == 2 and any("Verkauf" in b for b in badges) and any("FPS-Set" in b for b in badges),
              str(badges))
        await shot("m9f-inventory-breakdown.png")

        # Freitext-Zweck-Reservierung über das Panel (Titanium, Q720)
        check("2a R öffnet das Reservierungs-Panel", await js("clickRowR(2)") is True)
        await wait_for("resPanelOpen()", 5)
        check("2b Zweck-Text gesetzt", await js("setResPurpose('Projekt Bunker')") is True)
        await asyncio.sleep(0.2)
        await js("setResQty('10')")
        await asyncio.sleep(0.2)
        check("2c Speichern ruft reservation_add auf", await js("resSave()") is True)
        await wait_for("callsOf('reservation_add').length === 1", 8)
        calls = await js("callsOf('reservation_add')")
        ra = calls[-1]["args"]["new"]
        check("2d reservation_add: Freitext-Zweck, qty 10, inventory_id als String",
              ra["purpose"] == "Projekt Bunker" and ra["qty"] == 10
              and str(ra["inventory_id"]) == "3" and ra["list_id"] is None, str(ra))
        badges = await js("resBadges()")
        check("2e Badge erscheint live", any("Projekt Bunker" in b for b in badges), str(badges))
        subs = await js("qtySubs()")
        check("2f Breakdown aktualisiert (10 reserved / 30 free)",
              sorted(subs) == sorted(["6 reserved · 6 free", "10 reserved · 30 free"]), str(subs))
        await shot("m9f-inventory-reserve-panel.png")

        # Überbuchung fliegt (Rüber mit fehlender freier Menge)
        await js("clickRowR(0)")
        await wait_for("resPanelOpen()", 5)
        await js("setResPurpose('x')")
        await asyncio.sleep(0.2)
        await js("setResQty('99')")
        await asyncio.sleep(0.2)
        await js("resSave()")
        await asyncio.sleep(0.6)
        over_subs = await js("qtySubs()")
        check("2g Überbuchung abgewiesen (Breakdown unverändert)",
              sorted(over_subs) == sorted(["6 reserved · 6 free", "10 reserved · 30 free"]), str(over_subs))

        # Badge-✕ hebt die Freitext-Reservierung auf
        check("2h Badge-✕ ruft reservation_delete auf", await js("badgeX(1)") is True)
        await wait_for("callsOf('reservation_delete').length === 1", 8)
        dels = await js("callsOf('reservation_delete')")
        del_id = dels[0]["args"]["id"]
        check("2i reservation_delete mit Auftrags-Id", str(del_id) in ("902", "903"), str(del_id))
        await js("document.querySelector('.res-panel .res-cancel')?.click()")

        # ---- Gate 3-5: Crafting-Listen ----
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        if not await js("!!document.querySelector('.crafting')"):
            print("  DEBUG: no .crafting; body:", await js("document.body ? document.body.className + ' tabs:' + document.querySelectorAll('.ctab').length : 'NOBODY'"))
            print("  DEBUG ctabs:", await js("[...document.querySelectorAll('.ctabs,.ctab,.crafting,.rvw')].map(x=>x.className)"))
        await wait_for("craftLists() || true", 5)
        # Listen-Tab anspringen
        clicked = await js("""
          (() => { const b = [...document.querySelectorAll('.crafting .ctab')]
            .find((x) => x.textContent.trim() === 'Lists'); b?.click(); return !!b; })()
        """)
        await wait_for("craftLists()", 8)
        if not await js("craftLists()"):
            # Fallback: Chips erstetzen den Tab-Klick erst nach Listen-Anlage —
            # über Railway-Fallback: direkt prüfen, Probe wertet den CraftLists-Block weiter unten aus.
            pass
        check("3a Listen-Tab offen", await js("craftLists()") is True)
        check("3c Neue Liste anlegen", await js("setListName('Craft-Sperrliste')") is True)
        await asyncio.sleep(0.2)
        await js("clickNewList()")
        await wait_for("craftChips().length === 1", 8)
        # Eintrag mit 1 Material (Menge 50 SCU, Q 500)
        added = await js("""
          (async () => {
            await window.__TAURI_INTERNALS__.invoke('list_add_item', { listId: window.__LIST.lists[0].id,
              item: { item_id: 'r1', item_name: 'Rifle', craft_time: 60, copies: 1,
                      mats: [{ material: 'Gold', ore_id: null, unit: 'SCU', quantity: 50, quality: 500 }] } });
            // M9-K (P11): Liste über das ListPicker-Dropdown öffnen.
            document.querySelector('.clists .lp-btn')?.click();
            await new Promise((r) => setTimeout(r, 300));
            document.querySelector('.clists .lp-pop .lp-row')?.click();
            return true;
          })()
        """)
        await wait_for("aggTexts().length === 1", 8)
        aggs = await js("aggTexts()")
        check("3d Aggregationszeile 0/50 (noch keine Reservierung dieser Liste)",
              aggs and "0/50" in aggs[0] and not await js("resvChips()"), str(aggs))
        await shot("m9f-craft-reserved-chip.png")

        # ---- Gate 4: Aus Inventar reservieren ----
        check("4a Reserve-Button öffnet den Quellen-Picker", await js("clickReserveBtn()") is True)
        await wait_for("resPickRows().length === 2", 8)
        picks = await js("resPickRows()")
        check("4b Picker zeigt alle Quellen mit freien Mengen",
              len(picks) == 2 and any("Heim-Hangar" in p for p in picks)
              and any("Meine Carrack" in p for p in picks), str(picks))
        # Erste Quelle wählen (voller freier Bestand als Default = 6) + zweite teilw. 2
        await js("pickRow(0)")
        await asyncio.sleep(0.2)
        await js("setResPickQty('6')")
        await asyncio.sleep(0.2)
        await js("pickRow(1)")
        await asyncio.sleep(0.2)
        await js("setResPickQty('2')")
        await asyncio.sleep(0.2)
        check("4c Speichern über beide Quellen", await js("resPickSubmit()") is True)
        await wait_for("callsOf('reservation_add').length === 3", 10)
        adds = await js("callsOf('reservation_add')")
        two = adds[-2:]
        check("4d reservation_add mit list_id (Listen-Zweck), partiell",
              str(two[0]["args"]["new"]["qty"]) == "6" and str(two[1]["args"]["new"]["qty"]) == "2"
              and all(a["args"]["new"]["list_id"] for a in two), str(two))
        aggs = await js("aggTexts()")
        check("4e Reserviert 8 · Ø Q766 (mengengewichtet: 6×Q900 + 2×Q364)",
              aggs and any("reserved 8" in a and "Q766" in a for a in aggs), str(aggs))
        await shot("m9f-craft-reserve-picker.png")

        # ---- Gate 5: Inventar öffnen aus der Crafting-Liste ----
        check("5a „Inventar öffnen“ vorhanden", await js("clickOpenInventory()") is True)
        await wait_for("invRows() === 3", 8)
        await wait_for("document.querySelector('.inv-form')", 8)
        check("5b Inventar-Eingabemaske direkt offen",
              await js("!!document.querySelector('.inv-form')") is True)
        await shot("m9f-craft-open-inventory.png")

    server.terminate()
    proc.terminate()
    print(f"\nRESULT: PASS={PASS} FAIL={FAIL}")
    raise SystemExit(0 if FAIL == 0 else 1)


asyncio.run(main())
