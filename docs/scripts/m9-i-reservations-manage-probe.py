#!/usr/bin/env python3
"""M9-I probe (t_e7f0b3a6) — Reservierungs-Verwaltung aus der Liste.

Gates:
  1. Material-Zeile mit Reservierungen zeigt „reserviert X · Ø Q" CHIP +
     M9-P: EINE „Verwalten"-Aktion pro Zeile (konsolidiertes Panel).
  2. Verwalten öffnet das kombinierte Panel VORGEFÜLLT (je Quelle
     Menge + Q/Ort), Titel zeigt Bedarf/reserviert.
  3. Mengen-Änderung je Quelle → reservation_update (id + qty); Chip folgt.
  4. Löschen je Quelle (✕) → reservation_delete; Quellen-Zeile fällt in der
     Maske auf ungewählt zurück; Chip folgt nach dem Schließen.
  5. HARTER CAP: Reservieren über den Listenbedarf hinaus → err_list_cap
     (Stub spiegelt die serverseitige Cap-Logik), Fehler in der Maske;
     reservieren auf exakt den Bedarf klappt. Mengen-Schrumpfung → nächstes
     Listen-Öffnen zeigt den GEKLAMMTEN Stand (Stub spiegelt die Klemmung;
     Backend-Beleg = cargo tests m9i_*).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Ports 8857/9297 (Probe-Hotspot: pro Task eigene Ports).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_e7f0b3a6")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8857/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m9i-t7f0", ignore_errors=True)

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
  // ---- Inventar: Gold Q900 (reserviert 4 für die Liste) + Gold Q364 ----
  // list_id '1' = die im Probe-Flow erzeugte Liste (Stub-Seq startet bei 1).
  const INV = [
    { id: '1', entity_kind: 'commodity', entity_id: '1000', name: 'Gold', quality: 900,
      location_id: null, location_name: 'Heim-Hangar', quantity: 12, price: 4500,
      reservations: [],
      created_at: 0, updated_at: 0 },
    { id: '2', entity_kind: 'commodity', entity_id: '1001', name: 'Gold', quality: 364,
      location_id: null, location_name: 'Meine Carrack', quantity: 3, price: null,
      reservations: [], created_at: 0, updated_at: 0 }
  ];
  let invSeq = 100;

  // strata_items (Crafting-Hintergrund; smoke-stub stubbt es nicht — Muster m8-r1a)
  const ITEMS = { items: [
    { id: 'item-0', name: 'Plain Cannon', slug: 'plain-cannon', category: 'weapon' }
  ], count: 1, attribution: 'Data from celd.space / Strata Mining Tools' };

  // ---- Listen-Engine (Muster m9-c/m9-f) + M9-I Cap/Klemm-Spiegel ----
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
    if (cmd === 'inventory_locations') {
      const seen = new Set(); const names = [];
      for (const r of INV) { const k = r.location_name.toLowerCase(); if (!seen.has(k)) { seen.add(k); names.push(r.location_name); } }
      return names;
    }
    // M9-I-Helfer im Stub: Bedarf/Reserviert eines Materials einer Liste
    const reqOf = (listId, name) => {
      let req = 0;
      for (const it of DB.items.filter((i) => String(i.list_id) === String(listId))) {
        for (const m of DB.mats.filter((mm) => mm.item_row === it.id && mm.material.toLowerCase() === String(name).toLowerCase())) {
          req += m.quantity * it.copies;
        }
      }
      return req;
    };
    const resRowsOf = (listId, name) => {
      const rows = [];
      for (const r of INV) {
        for (const res of r.reservations) {
          if (String(res.list_id) === String(listId) && r.name.toLowerCase() === String(name).toLowerCase()) {
            rows.push({ row: r, res });
          }
        }
      }
      rows.sort((x, y) => Number(y.res.id) - Number(x.res.id)); // jüngste zuerst
      return rows;
    };
    const clampMaterial = (listId, name) => {
      const req = reqOf(listId, name);
      let sum = resRowsOf(listId, name).reduce((s, x) => s + x.res.qty, 0);
      for (const x of resRowsOf(listId, name)) {
        if (sum <= req + 1e-9) break;
        const excess = Math.min(sum - req, x.res.qty);
        const keep = x.res.qty - excess;
        if (keep <= 1e-9) {
          x.row.reservations.splice(x.row.reservations.indexOf(x.res), 1);
        } else {
          x.res.qty = keep;
        }
        sum -= excess;
      }
      return req;
    };
    if (cmd === 'reservation_add') {
      const n = a.new;
      const inv = INV.find((r) => String(r.id) === String(n.inventory_id));
      if (!inv) throw new Error('Position ' + n.inventory_id + ' existiert nicht');
      if (!n.purpose.trim() && !n.list_id) throw new Error('reservations.err_purpose');
      if (n.list_id) {
        clampMaterial(n.list_id, inv.name);
        const req = reqOf(n.list_id, inv.name);
        const res = resRowsOf(n.list_id, inv.name).reduce((s, x) => s + x.res.qty, 0);
        if (n.qty > (req - res) + 1e-9) {
          throw new Error(JSON.stringify({ error_key: 'reservations.err_list_cap', params: { req, res } }));
        }
      }
      const free = inv.quantity - inv.reservations.reduce((s, x) => s + x.qty, 0);
      if (!(n.qty > 0) || n.qty > free + 1e-9) throw new Error('reservations.err_no_free');
      const res = { id: String(++invSeq), qty: n.qty, purpose: n.purpose || '',
                    list_id: n.list_id ? String(n.list_id) : null,
                    list_name: null, note: n.note || '' };
      inv.reservations.push(res);
      window.__LAST_RES_ID = res.id;
      return res;
    }
    if (cmd === 'reservation_update') {
      for (const r of INV) {
        const res = r.reservations.find((x) => String(x.id) === String(a.id));
        if (res) {
          if (!(a.qty > 0)) throw new Error('inventory.err_qty');
          if (res.list_id) {
            const req = reqOf(res.list_id, r.name);
            const others = resRowsOf(res.list_id, r.name).filter((x) => String(x.res.id) !== String(a.id)).reduce((s, x) => s + x.res.qty, 0);
            if (a.qty > (req - others) + 1e-9) {
              throw new Error(JSON.stringify({ error_key: 'reservations.err_list_cap', params: { req, res: others + res.qty } }));
            }
          }
          const othersPos = r.reservations.filter((x) => String(x.id) !== String(a.id)).reduce((s, x) => s + x.qty, 0);
          if (a.qty > (r.quantity - othersPos) + 1e-9) throw new Error('reservations.err_no_free');
          res.qty = a.qty;
          return null;
        }
      }
      throw new Error('reservations.err_missing');
    }
    if (cmd === 'reservation_delete') {
      for (const r of INV) {
        const i = r.reservations.findIndex((x) => String(x.id) === String(a.id));
        if (i >= 0) { r.reservations.splice(i, 1); return null; }
      }
      throw new Error('reservations.err_missing');
    }
    if (cmd === 'lists_list') return DB.lists.filter((l) => l.kind === a.kind).map((l) => ({
      id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
      item_count: DB.items.filter((i) => String(i.list_id) === String(l.id)).length }));
    if (cmd === 'list_create') {
      const id = DB.seq++;
      DB.lists.push({ id, kind: a.kind, name: a.name, created_at: Math.floor(Date.now() / 1000) });
      return id;
    }
    if (cmd === 'list_add_item') {
      const it = a.item;
      const row = DB.seq++;
      DB.items.push({ id: row, list_id: String(a.listId), item_id: it.item_id, item_name: it.item_name,
                      craft_time: it.craft_time, copies: it.copies, created_at: 1 });
      for (const m of it.mats) DB.mats.push({ item_row: row, material: m.material.trim(),
        ore_id: m.ore_id, unit: m.unit, quantity: m.quantity, quality: m.quality });
      return row;
    }
    if (cmd === 'list_set_copies') {
      const it = DB.items.find((i) => i.id === a.itemRow);
      if (!it) throw new Error('lists.err_item_missing');
      it.copies = a.copies;
      return null;
    }
    if (cmd === 'list_set_done') {
      DB.done = DB.done.filter((d) => !(d.list_id === a.listId && d.material === a.material && d.unit === a.unit));
      if (a.doneQty != null) DB.done.push({ list_id: a.listId, material: a.material, unit: a.unit, done_qty: a.doneQty });
      return null;
    }
    if (cmd === 'list_detail') {
      const l = DB.lists.find((x) => x.id === a.listId);
      if (!l) throw new Error('lists.err_missing');
      // M9-I: Listen-Öffnen klemmt (Stub spiegelt die Backend-Klemmung).
      for (const r of INV) for (const res of [...r.reservations]) {
        if (String(res.list_id) === String(l.id)) clampMaterial(l.id, r.name);
      }
      const map = new Map();
      for (const it of DB.items.filter((i) => String(i.list_id) === String(l.id))) {
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
          const k = r.name + ':' + (map.get(r.name + ':gems') ? 'gems' : 'SCU');
          const e = map.get(k) || map.get(r.name + ':SCU');
          if (e) {
            const w = e.reserved_qty * (e.reserved_avg_quality ?? 0);
            e.reserved_qty += res.qty;
            const qual = (e.reserved_avg_quality == null ? r.quality : e.reserved_avg_quality);
            e.reserved_avg_quality = Math.round((w + res.qty * r.quality) / e.reserved_qty);
          }
        }
      }
      return { id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
        items: DB.items.filter((i) => String(i.list_id) === String(l.id)).map((i) => ({ ...i, mats: DB.mats.filter((mm) => mm.item_row === i.id) })),
        aggregated: [...map.values()].sort((a, b) => a.material.localeCompare(b.material)) };
    }
    return orig(cmd, args);
  };

  // ---- Probe-Helfer ----
  window.pressKey = (key) => window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  const q = (s) => document.querySelector(s);
  const qa = (s) => [...document.querySelectorAll(s)];
  window.craftLists = () => !!q('.clists');
  window.setListName = (v) => {
    const i = q('.clists .cl-input');
    if (!i) return false;
    i.value = v; i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.clickNewList = () => { const b = [...qa('.clists .cl-btn')][0]; b?.click(); return !!b; };
  window.aggTexts = () => qa('.clists .cl-agg-row').map((e) => e.textContent.replace(/\s+/g, ' ').trim());
  window.resvChips = () => qa('.clists .cl-resv').map((e) => e.textContent.trim());
  window.clickManageBtn = () => {
    // M9-P: EIN Button pro Zeile — das kombinierte Panel (Verwalten).
    const b = [...qa('.clists .cl-agg-actions .cl-mini')].find((x) => x.textContent.includes('Manage') || x.textContent.includes('Verwalten'));
    b?.click();
    return !!b;
  };
  window.resMaskTitle = () => q('.clists .cl-inv-title')?.textContent.replace(/\s+/g, ' ').trim() ?? null;
  window.resPickRows = () => qa('.clists .cl-res-pick').map((e) => ({ on: e.classList.contains('is-on'), text: e.textContent.replace(/\s+/g, ' ').trim() }));
  window.pickRow = (idx) => {
    const b = qa('.clists .cl-res-pick')[idx];
    b?.click();
    return !!b;
  };
  window.resQtyInputs = () => qa('.clists .cl-res-qty').map((i) => i.value);
  window.setResQtyIdx = (idx, v) => {
    const inputs = qa('.clists .cl-res-qty');
    const i = inputs[idx];
    if (!i) return false;
    i.value = v; i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.resDelBtns = () => qa('.clists .cl-res-del').length;
  window.clickResDel = (idx) => {
    const b = qa('.clists .cl-res-del')[idx];
    b?.click();
    return !!b;
  };
  window.resMaskError = () => q('.clists .cl-inv-form .cl-err')?.textContent.trim() ?? null;
  window.resSubmit = () => {
    // M9-P: Panel hat mehrere cl-btn (Alles reservieren, Reserve, Speichern) —
    // exakt den Speichern-Button treffen.
    const b = [...qa('.clists .cl-inv-form .cl-btn')].find((x) => x.textContent.trim() === 'Save' || x.textContent.trim() === 'Speichern');
    b?.click();
    return !!b;
  };
  window.resCancel = () => { const b = [...qa('.clists .cl-inv-form .cl-mini')].find((x) => x.textContent.includes('Cancel') || x.textContent.includes('Abbrechen')); b?.click(); return !!b; };
  window.actionBtnCount = () => qa('.clists .cl-agg-actions .cl-mini').length;
  // M9-K (P11): Chips → ListPicker-Dropdown. Eine Zeile picken = Liste öffnen
  // (ersetzt die alten .cl-chip-Klicks).
  window.pickList = (idx) => {
    q('.clists .lp-btn')?.click();
    // Listenwechsel rendert asynchron — 2× RAF, bevor die Zeile geklickt
    // wird (Gate-6a-Fix: sonst geht der Klick auf die alte Zeile ins Leere
    // und list_set_copies/list_detail laufen gegen den alten Stand).
    return new Promise((res) => {
      requestAnimationFrame(() => requestAnimationFrame(() => {
        const rows = qa('.clists .lp-row');
        rows[idx]?.click();
        res(!!rows[idx]);
      }));
    });
  };
  window.callsOf = (cmd) => window.__CALLS.filter((c) => c.cmd === cmd);
})();
"""


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8857", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9297", "--user-data-dir=/tmp/cdp-m9i-t7f0",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9297/json/list", timeout=1) as r:
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

        # ---- Listen aufsetzen (Muster m9-f): Crafting → Lists ----
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("""
          (() => { const b = [...document.querySelectorAll('.crafting .ctab')]
            .find((x) => x.textContent.trim() === 'Lists'); b?.click(); return !!b; })()
        """)
        await wait_for("craftLists()", 8)
        check("0a Listen-Tab offen", await js("craftLists()") is True)
        await js("setListName('M9I-Set')")
        await asyncio.sleep(0.2)
        await js("clickNewList()")
        await wait_for("callsOf('list_create').length === 1", 8)
        await js("""
          (async () => {
            await window.__TAURI_INTERNALS__.invoke('list_add_item', { listId: window.__LIST.lists[0].id,
              item: { item_id: 'r1', item_name: 'Rifle', craft_time: 60, copies: 1,
                      mats: [{ material: 'Gold', ore_id: null, unit: 'SCU', quantity: 10, quality: 500 }] } });
            const ok = window.pickList ? window.pickList(0) : false;
            return ok;
          })()
        """)
        await wait_for("aggTexts().length === 1", 8)
        aggs = await js("aggTexts()")
        check("0b Aggregationszeile 0/10 (noch ohne Reservierung)",
              aggs and "0/10" in aggs[0] and not await js("resvChips()"), str(aggs))
        # Ist-Stand herstellen: Reservierung 4 aus Gold Q900 über die Maske-
        # API (der Cap lässt das zu: Bedarf 10). Ein VOR dem Item-Add
        # gepflanzter Stand würde beim Öffnen korrekt auf Bedarf 0 geklemmt.
        await js("""
          (async () => {
            await window.__TAURI_INTERNALS__.invoke('reservation_add', {
              'new': { inventory_id: '1', list_id: '1', purpose: '', qty: 4, note: 'Gold' } });
            return window.pickList ? window.pickList(0) : false;
          })()
        """)
        await wait_for("resvChips().length === 1", 8)
        aggs = await js("aggTexts()")
        check("1a Chip zeigt reserviert 4 · Ø Q900 (Ist-Stand)",
              aggs and any("reserved 4" in a and "Q900" in a for a in aggs), str(aggs))
        check("1b Manage-Aktion vorhanden (M9-P: EIN Button pro Zeile)",
              await js("clickManageBtn()") is True)
        await shot("m9i-edit-action.png")

        # ---- Gate 2: Maske vorgefüllt ----
        await wait_for("resPickRows().length === 2", 8)
        title = await js("resMaskTitle()")
        check("2a Titel zeigt Bedarf/reserviert (10 / 4)",
              title and "10" in title and "4" in title, str(title))
        picks = await js("resPickRows()")
        check("2b Quelle 1 ist gewählt (vorgefüllt), Quelle 2 nicht",
              picks and picks[0]["on"] and not picks[1]["on"], str(picks))
        check("2c Input vorgefüllt mit 4", await js("resQtyInputs()") == ["4"], str(await js("resQtyInputs()")))
        check("2d ✕ je reservierter Quelle vorhanden", await js("resDelBtns()") == 1)
        await shot("m9i-edit-mask.png")

        # ---- Gate 3: Mengen-Änderung → reservation_update ----
        await js("setResQtyIdx(0, '2')")
        await asyncio.sleep(0.2)
        check("3a Speichern", await js("resSubmit()") is True)
        await wait_for("callsOf('reservation_update').length === 1", 8)
        ups = await js("callsOf('reservation_update')")
        res_id = await js("window.__LAST_RES_ID")
        check("3b reservation_update(Reservierungs-Id, qty 2)",
              ups and str(ups[0]["args"]["id"]) == str(res_id) and ups[0]["args"]["qty"] == 2, str(ups))
        await wait_for("resvChips().join('').includes('reserved 2')", 8)
        aggs = await js("aggTexts()")
        check("3c Chip folgt: reserved 2", aggs and any("reserved 2" in a for a in aggs), str(aggs))

        # ---- Gate 4: Löschen je Quelle ----
        check("4a Verwalten erneut öffnet das Panel", await js("clickManageBtn()") is True)
        await wait_for("resQtyInputs().length === 1", 8)
        check("4b Ist-Stand vorgefüllt (2)", await js("resQtyInputs()") == ["2"], str(await js("resQtyInputs()")))
        check("4c ✕ löscht die Quelle", await js("clickResDel(0)") is True)
        await wait_for("callsOf('reservation_delete').length === 1", 8)
        dels = await js("callsOf('reservation_delete')")
        check("4d reservation_delete(Reservierungs-Id)",
              dels and str(dels[0]["args"]["id"]) == str(res_id), str(dels))
        await wait_for("resPickRows().length === 2 && resDelBtns() === 0", 8)
        picks = await js("resPickRows()")
        check("4e Quelle nach Löschen ungewählt, kein ✕ mehr",
              picks and not picks[0]["on"] and not picks[1]["on"], str(picks))
        await js("resCancel()")
        await wait_for("!resMaskTitle()", 5)
        aggs = await js("aggTexts()")
        check("4f Chip weg (reserviert 0)", aggs and not await js("resvChips()"), str(aggs))
        # M9-P: Die Action-Zeile hat GENAU EINEN Button (konsolidiertes Panel).
        check("4g M9-P: genau EIN Action-Button pro Zeile",
              await js("actionBtnCount()") == 1, str(await js("actionBtnCount()")))

        # ---- Gate 5: Harter Cap beim Reservieren ----
        check("5a Manage-Button öffnet den Picker", await js("clickManageBtn()") is True)
        await wait_for("resPickRows().length === 2", 8)
        await js("pickRow(0)")
        await asyncio.sleep(0.2)
        await js("setResQtyIdx(0, '12')")
        await asyncio.sleep(0.2)
        check("5b Über Bedarf (12 > 10) speichern — Position hätte frei 12", await js("resSubmit()") is True)
        await wait_for("resMaskError() !== null", 8)
        err = await js("resMaskError()")
        check("5c err_list_cap in der Maske (Bedarf 10, reserviert 0)",
              err and "List cap" in err and "10" in err and "12" not in err.split("requirement")[1].split(",")[0], str(err))
        await shot("m9i-cap-error.png")
        await js("setResQtyIdx(0, '10')")
        await asyncio.sleep(0.2)
        check("5d Exakt am Bedarf reservieren (10 ≤ frei 12)", await js("resSubmit()") is True)
        await wait_for("callsOf('reservation_add').length === 2", 8)
        adds = await js("callsOf('reservation_add')")
        na = adds[-1]["args"]["new"]
        check("5e reservation_add(list_id, qty 10)", str(na["list_id"]) == "1" and na["qty"] == 10, str(na))
        await wait_for("resvChips().join('').includes('reserved 10')", 8)
        aggs = await js("aggTexts()")
        check("5f Chip: reserved 10 · Ø Q900", aggs and any("reserved 10" in a and "Q900" in a for a in aggs), str(aggs))
        await shot("m9i-reserved-at-cap.png")

        # ---- Gate 6: Schrumpfung → Klemmung beim nächsten Öffnen ----
        await js("""
          (async () => {
            await window.__TAURI_INTERNALS__.invoke('list_set_copies', { itemRow: window.__LIST.items[0].id, copies: 0.5 });
            return window.pickList ? window.pickList(0) : false;
          })()
        """)
        await wait_for("aggTexts().join('').includes('0/25')", 8)
        await wait_for("resvChips().join('').includes('reserved 25')", 8)
        aggs = await js("aggTexts()")
        check("6a Bedarf 5, reserviert geklemmt auf 5",
              aggs and any("0/5" in a for a in aggs) and any("reserved 5" in a for a in aggs), str(aggs))
        await shot("m9i-clamped-after-shrink.png")

    server.terminate()
    proc.terminate()
    print(f"\nRESULT: PASS={PASS} FAIL={FAIL}")
    raise SystemExit(0 if FAIL == 0 else 1)


asyncio.run(main())
