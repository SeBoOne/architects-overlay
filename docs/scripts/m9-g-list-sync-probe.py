#!/usr/bin/env python3
"""M9-G probe (t_131614e0) — Verknüpfungs-Sync (Welle 2, Punkte 2+5+7).

Gates:
  1. Standalone Mining-Liste: Add-Formular + Mengen-Stepper sichtbar (kein Lock).
  2. Verknüpfte Mining-Liste (source_list_id): KEIN Add-Formular, keine
     Stepper/Entfernen-Buttons, Hinweis „linked_readonly“ mit dem Namen der
     Crafting-Liste — Material/Mengen read-only (M9-P: Abhaken entfällt,
     Status = done+reserved).
  3. Craft→Mining-Sync: Mengen-Änderung an der Crafting-Liste (list_set_copies)
     spiegelt sich automatisch in der verknüpften Mining-Liste (Aggregation).
  4. Craft→Mining-Sync: Entfernen eines Craft-Eintrags nimmt das Material aus
     der verknüpften Mining-Liste (inkl. done-Stand).
  5. Mining→Craft: Inventar-Add über die verknüpfte Mining-Liste reserviert
     (M9-P: kein list_set_done mehr) — die Reservierung zählt LISTENFAMILIEN-
     WEIT und schlägt auf die Crafting-Liste durch (list_detail M9-P).
  6. Unverknüpfte Listen: Reservierung entfällt — Inventory-Flow nutzt
     inventory_add (kein listId-Argument) für standalone Listen (Spec Punkt 5,
     seit M9-F implementiert — hier als IPC-Beleg).
  7. Deutsch: Hinweistext der verknüpften Liste (i18n-Parität live).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Der Stub implementiert die Sync-Semantik des Rust-Backends (Spiegel auf
source_list_id) 1:1 in JS. Ports 8899/9247 (Probe-Hotspot: pro Task eigene).
"""
import asyncio, json, os, pathlib, urllib.request, shutil

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_131614e0")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8899/"

PASS = 0
FAIL = 0

shutil.rmtree("/tmp/cdp-m9g-sync", ignore_errors=True)

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
  const ORES = { ores: [
    { id: 'ore-hadanite', name: 'Hadanite', slug: 'hadanite', category: 'ship', tier: 'uncommon',
      scanSignature: 12, instability: 0.3, resistance: 0.4 },
    { id: 'ore-taranite', name: 'Taranite (Raw)', slug: 'taranite-raw', category: 'ship', tier: 'rare',
      scanSignature: 9, instability: 0.5, resistance: 0.2 }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };
  const LOCS = { locations: [], count: 0, attribution: 'x' };

  // ---- In-Memory-Listen-Engine (Spiegel des Rust-Backends, M9-G) ----
  const DB = { seq: 100, lists: [], items: [], mats: [], done: [], res: [] };
  function aggOf(listId) {
    const map = new Map();
    for (const it of DB.items.filter((i) => i.list_id === listId)) {
      for (const m of DB.mats.filter((mm) => mm.item_row === it.id)) {
        const k = m.material + ':' + m.unit;
        const e = map.get(k) || { material: m.material, unit: m.unit, quantity: 0, min_quality: 0 };
        e.quantity += m.quantity * it.copies;
        e.min_quality = Math.max(e.min_quality, m.quality);
        map.set(k, e);
      }
    }
    return [...map.values()].sort((a, b) => a.material.localeCompare(b.material));
  }
  function doneOf(listId) {
    const m = new Map();
    for (const d of DB.done.filter((d) => d.list_id === listId)) m.set(d.material + ':' + d.unit, d.done_qty);
    return m;
  }
  // Spiegel: Crafting-Liste → alle verknüpften Mining-Listen (Rust: sync_linked_mining).
  function syncLinked(sourceId) {
    const agg = aggOf(sourceId);
    for (const ml of DB.lists.filter((l) => l.source_list_id === sourceId)) {
      const done = doneOf(ml.id);
      DB.mats = DB.mats.filter((mm) => mm.item_row < 0 || !DB.items.some((i) => i.id === mm.item_row && i.list_id === ml.id));
      DB.items = DB.items.filter((i) => i.list_id !== ml.id);
      DB.done = DB.done.filter((d) => d.list_id !== ml.id);
      for (const a of agg) {
        if (a.quantity <= 0) continue;
        const row = DB.seq++;
        DB.items.push({ id: row, list_id: ml.id, item_id: 'ore-hadanite', item_name: a.material,
                        craft_time: 0, copies: a.quantity, created_at: 1 });
        DB.mats.push({ item_row: row, material: a.material, ore_id: null, unit: a.unit, quantity: 1, quality: a.min_quality });
      }
      for (const [k, dq] of done) {
        const still = agg.find((a) => a.material + ':' + a.unit === k && a.quantity > 0);
        if (!still) continue;
        DB.done.push({ list_id: ml.id, material: still.material, unit: still.unit, done_qty: Math.min(dq, still.quantity) });
      }
    }
  }
  function detailOf(listId) {
    const l = DB.lists.find((x) => x.id === listId);
    if (!l) throw new Error('lists.err_missing');
    const dm = doneOf(listId);
    // M9-P (t_fb405e8d): Reservierungen zählen LISTENFAMILIENWEIT (Rust:
    // list_detail reserved_map über self + Kinder + Quelle).
    const fam = familyIds(listId);
    return {
      id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
      items: DB.items.filter((i) => i.list_id === listId).map((i) => ({
        ...i, mats: DB.mats.filter((mm) => mm.item_row === i.id)
      })),
      aggregated: aggOf(listId).map((a) => {
        const rs = DB.res.filter((r) => fam.includes(r.list_id)
          && r.material.toLowerCase() === a.material.toLowerCase());
        const rq = rs.reduce((s, r) => s + r.qty, 0);
        // M9-M: Fortschritt = (done + reserved) / Bedarf, zentral in list_detail.
        const dq = dm.get(a.material + ':' + a.unit) || 0;
        const prog = a.quantity > 0 ? Math.min(100.0, (100.0 * (dq + rq)) / a.quantity) : 0.0;
        return { ...a, done_qty: dq,
                 reserved_qty: rq, reserved_avg_quality: rq > 0 ? 500 : null,
                 progress: Math.round(prog * 1e6) / 1e6, reached: dq + rq >= a.quantity && a.quantity > 0 };
      })
    };
  }
  function familyIds(id) {
    const out = new Set([id]);
    for (const l of DB.lists) {
      if (l.source_list_id === id) out.add(l.id);
      if (l.id === id && l.source_list_id != null) out.add(l.source_list_id);
    }
    return [...out];
  }

  // Seed: Craft-Liste 1 (2× Hadanite Q500) + verknüpfte Mining-Liste 2.
  DB.lists.push({ id: 1, kind: 'craft', name: 'Craft-Quelle', created_at: 1, source_list_id: null });
  DB.items.push({ id: 10, list_id: 1, item_id: 'r1', item_name: 'Rifle', craft_time: 120, copies: 2, created_at: 1 });
  DB.mats.push({ item_row: 10, material: 'Hadanite', ore_id: 'ore-hadanite', unit: 'gems', quantity: 100, quality: 500 });
  DB.lists.push({ id: 2, kind: 'mining', name: 'Mine', created_at: 2, source_list_id: 1 });
  syncLinked(1);

  window.__CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    window.__CALLS.push({ cmd, args: a });
    if (cmd === 'settings_get' && window.__M9G_LANG === 'de') {
      const cfg = await orig(cmd, args);
      return { ...(cfg || {}), language: 'de' };
    }
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4, scwiki_ready: true };
    if (cmd === 'strata_ores') return ORES;
    if (cmd === 'strata_locations') return LOCS;
    if (cmd === 'lists_list') return DB.lists.filter((l) => l.kind === a.kind).map((l) => ({
      id: l.id, kind: l.kind, name: l.name, created_at: l.created_at,
      item_count: DB.items.filter((i) => i.list_id === l.id).length,
      source_list_id: l.source_list_id,
      source_list_name: l.source_list_id ? DB.lists.find((x) => x.id === l.source_list_id).name : null }));
    if (cmd === 'list_create') {
      const id = DB.seq++;
      DB.lists.push({ id, kind: a.kind, name: a.name, created_at: Math.floor(Date.now() / 1000), source_list_id: a.sourceListId ?? null });
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
      const row = DB.seq++;
      DB.items.push({ id: row, list_id: a.listId, item_id: it.item_id, item_name: it.item_name,
                      craft_time: it.craft_time, copies: it.copies, created_at: 1 });
      for (const m of it.mats) DB.mats.push({ item_row: row, material: m.material.trim(),
        ore_id: m.ore_id, unit: m.unit, quantity: m.quantity, quality: m.quality });
      syncLinked(a.listId);
      return row;
    }
    if (cmd === 'list_remove_item') {
      const row = DB.items.find((i) => i.id === a.itemRow);
      DB.mats = DB.mats.filter((m) => m.item_row !== a.itemRow);
      DB.items = DB.items.filter((i) => i.id !== a.itemRow);
      if (row) syncLinked(row.list_id);
      return null;
    }
    if (cmd === 'list_set_copies') {
      const row = DB.items.find((i) => i.id === a.itemRow);
      row.copies = a.copies;
      syncLinked(row.list_id);
      return null;
    }
    if (cmd === 'list_set_done') {
      DB.done = DB.done.filter((d) => !(d.list_id === a.listId && d.material === a.material && d.unit === a.unit));
      if (a.doneQty != null) DB.done.push({ list_id: a.listId, material: a.material, unit: a.unit, done_qty: a.doneQty });
      const l = DB.lists.find((x) => x.id === a.listId);
      // Rust: Abhaken in verknüpfter Mining-Liste schlägt auf die Quelle durch.
      if (l && l.kind === 'mining' && l.source_list_id) {
        const src = l.source_list_id;
        DB.done = DB.done.filter((d) => !(d.list_id === src && d.material === a.material && d.unit === a.unit));
        if (a.doneQty != null) DB.done.push({ list_id: src, material: a.material, unit: a.unit, done_qty: a.doneQty });
      }
      return null;
    }
    if (cmd === 'list_detail') return detailOf(a.listId);
    if (cmd === 'inventory_add_for_list') {
      window.__CALLS.push({ cmd: '__RESERVED__', args: a });
      // Rust-Mirror (M9-E/M9-I): Reservierung wird auf den freien RESTBEDARF
      // der eigenen Liste geklemmt (harter Cap).
      const req = aggOf(a.listId).filter((x) => x.material.toLowerCase() === String(a.material).toLowerCase())
        .reduce((s, x) => s + x.quantity, 0);
      const own = DB.res.filter((r) => r.list_id === a.listId
        && r.material.toLowerCase() === String(a.material).toLowerCase()).reduce((s, r) => s + r.qty, 0);
      const q = Math.max(0, Math.min(a.entry.quantity, req - own));
      if (q > 0) DB.res.push({ list_id: a.listId, material: a.material, qty: q });
      return { entry: { name: a.entry.name, quality: a.entry.quality, location_name: a.entry.location_name } };
    }
    if (cmd === 'inventory_add') {
      window.__CALLS.push({ cmd: '__NOT_RESERVED__', args: a });
      return { name: a.entry.name, quality: a.entry.quality, location_name: a.entry.location_name };
    }
    return orig(cmd, args);
  };

  // ---- Probe-Helfer ----
  window.pressKey = (key) => window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  window.clickTab = (label) => {
    const b = [...document.querySelectorAll('.mining .mtab')].find((x) => x.textContent.trim() === label);
    if (!b) return false;
    b.click();
    return true;
  };
  const q = (s) => document.querySelector(s);
  window.listRegion = () => !!q('.mining .mlists');
  // M9-K (t_db9b234f, P11): Chips durch ListPicker-Dropdown ersetzt —
  // selectChip öffnet das Popover und wählt die Zeile; chips() liest die
  // Popover-Zeilen (und liefert [Label], wenn zu).
  window.selectChip = (name) => {
    const btn = document.querySelector('.mining .lp-btn');
    if (!btn) return false;
    btn.click();
    return new Promise((resolve) => setTimeout(() => {
      const row = [...document.querySelectorAll('.mining .lp-pop .lp-row')]
        .find((c) => c.textContent.trim().startsWith(name));
      if (!row) { resolve(false); return; }
      row.click();
      resolve(true);
    }, 250));
  };
  window.chips = () => {
    const label = document.querySelector('.mining .lp-btn .lp-label')?.textContent.trim();
    return label ? [label] : [];
  };
  window.setListName = (v) => {
    const i = q('.mining .ml-new .ml-input');
    if (!i) return false;
    i.value = v; i.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  };
  window.clickNewList = () => { const b = [...document.querySelectorAll('.mining .ml-new .ml-btn')][0]; b?.click(); return !!b; };
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
    if (!b) return false; b.click(); return true;
  };
  window.clickAdd = () => {
    const b = [...document.querySelectorAll('.mining .ml-add .ml-btn')].find((x) => !x.disabled);
    if (!b) return false; b.click(); return true;
  };
  window.itemRows = () => [...document.querySelectorAll('.mining .ml-item')].map((r) => r.textContent.trim());
  window.aggRows = () => [...document.querySelectorAll('.mining .ml-agg-row')].map((r) => r.textContent.trim());
  window.addFormVisible = () => !!q('.mining .ml-add');
  window.stepButtons = () => document.querySelectorAll('.mining .ml-item .ml-step').length;
  window.removeButtons = () => [...document.querySelectorAll('.mining .ml-item .ml-mini')].length;
  window.lockHint = () => q('.mining .ml-hint')?.textContent.trim() ?? null;
  window.invOpenAt = (idx) => {
    const rows = document.querySelectorAll('.mining .ml-agg-row');
    const bs = [...(rows[idx]?.querySelectorAll('.ml-agg-actions button') || [])];
    const b = bs.find((x) => /inventory|Inventar/.test(x.textContent));
    if (!b) return false; b.click(); return true;
  };
  window.invFormVisible = () => !!q('.mining .ml-inv-form');
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
  window.callsOf = (cmd) => window.__CALLS.filter((c) => c.cmd === cmd);
  window.inv = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args);
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
        "--remote-debugging-port=9247", "--user-data-dir=/tmp/cdp-m9g-sync",
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

    async with websockets_connect(ws) as cdp:
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

        VK = {"F5": 116, "Escape": 27, "Enter": 13}

        async def key(k):
            vk = VK[k]
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.4)

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

        # Mining-Modus → Listen-Tab
        await key("F5")
        await wait_for("document.querySelector('.mining')", 8)
        await js("clickTab('Lists')")
        await wait_for("listRegion()", 8)

        # ---- Gate 1: Seed (verknüpfte Liste) + standalone Liste ----
        await wait_for("chips().length === 1", 8)
        chips = await js("chips()")
        check("1a Seed: verknüpfte Mining-Liste 'Mine'", chips and chips[0].startswith("Mine"), str(chips))
        # Öffne die verknüpfte Liste für Gate 2 direkt.
        await js("selectChip('Mine')")
        await asyncio.sleep(0.3)
        title = await js("document.querySelector('.mining .ml-title')?.textContent")
        check("1b Verknüpfte Liste geöffnet", title == "Mine", str(title))
        check("1c KEIN Add-Formular (verknüpft)", await js("addFormVisible()") is False)
        check("1d KEINE Mengen-Stepper (verknüpft)", (await js("stepButtons()") or 0) == 0)
        check("1e KEIN Entfernen-Button (verknüpft)", (await js("removeButtons()") or 0) == 0)
        hint = await js("lockHint()")
        check("1f Read-only-Hinweis mit Quell-Name", hint is not None and "Craft-Quelle" in hint, str(hint))
        agg = await js("aggRows()")
        check("1g Spiegel-Inhalt: 0/200 Hadanite", agg and "0/200" in agg[0], str(agg))
        await shot("m9g-locked.png")

        # ---- Gate 2: Standalone-Liste bleibt editierbar ----
        await js("setListName('Solo-Run')")
        await asyncio.sleep(0.2)
        await js("clickNewList()")
        # M9-K (P11): Dropdown listet ALLE Listen — Anzahl im Popover prüfen.
        await js("document.querySelector('.mining .lp-btn')?.click()")
        await wait_for("document.querySelectorAll('.mining .lp-pop .lp-row').length === 2", 8)
        await js("document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, clientX: 0, clientY: document.querySelector('.mining').getBoundingClientRect().top + 400 }))")
        await asyncio.sleep(0.3)
        await js("selectChip('Solo')")
        await asyncio.sleep(0.3)
        check("2a Add-Formular sichtbar (standalone)", await js("addFormVisible()") is True)
        await js("setAdd('had', '250', '800')")
        await asyncio.sleep(0.2)
        sug = await js("suggestNames()")
        check("2b Typeahead filtert auf Hadanite", sug == ["Hadanite"], str(sug))
        await js("clickSuggest('Hadanite')")
        await asyncio.sleep(0.2)
        await js("clickAdd()")
        await wait_for("itemRows().length === 1", 8)
        check("2c Mengen-Stepper sichtbar (standalone)", (await js("stepButtons()") or 0) == 2)
        check("2d KEIN read-only-Hinweis (standalone)", await js("lockHint()") is None)
        await shot("m9g-standalone.png")

        # ---- Gate 3: Craft-Mengen-Änderung spiegelt sich ----
        await js("inv('list_set_copies', { itemRow: 10, copies: 4 })")
        await asyncio.sleep(0.3)
        await js("selectChip('Mine')")
        await asyncio.sleep(0.3)
        agg = await js("aggRows()")
        check("3a Spiegel folgt: 0/400", agg and "0/400" in agg[0], str(agg))

        # ---- Gate 4: Craft-Entfernen spiegelt sich ----
        await js("inv('list_remove_item', { itemRow: 10 })")
        await asyncio.sleep(0.3)
        await js("selectChip('Mine')")
        await asyncio.sleep(0.3)
        rows = await js("itemRows()")
        agg = await js("aggRows()")
        check("4a Spiegel leer nach Craft-Entfernen", rows == [] and agg == [], f"{rows} / {agg}")
        # Material zurücksetzen für Gate 5: neu anlegen (500) via invoke.
        await js("""inv('list_add_item', { listId: 1, item: {
            item_id: 'r1', item_name: 'Rifle', craft_time: 120, copies: 2,
            mats: [{ material: 'Hadanite', ore_id: 'ore-hadanite', unit: 'gems', quantity: 100, quality: 500 }] } })""")
        await asyncio.sleep(0.3)
        await js("selectChip('Mine')")
        await asyncio.sleep(0.3)
        rows = await js("aggRows()")
        check("4b Spiegel erneut 200 nach Wiedereintrag", rows and "0/200" in rows[0], str(rows))
        await shot("m9g-synced.png")

        # ---- Gate 5: Mining-Reservierung schlägt auf Craft durch ----
        # M9-P (P7): UI-Pfad ist NUR „Add to inventory" mit Vorbefüllung
        # (Rest) → submit reserviert (inventory_add_for_list, Cap auf den
        # Restbedarf); `list_set_done` wird NICHT mehr gerufen — der
        # Fortschritt zählt die Reservierung (M9-M: done + reserved).
        await js("invOpenAt(0)")
        await wait_for("invFormVisible()", 5)
        await js("invSet('Depot', '500', null)")
        await asyncio.sleep(0.2)
        await js("invSubmit()")
        await asyncio.sleep(0.5)
        craft = await js("inv('list_detail', { listId: 1 })") or {"aggregated": []}
        had = [m for m in craft["aggregated"] if m["material"] == "Hadanite"][0]
        check("5a Reservierung 200 zählt als Fortschritt (M9-P: kein set_done)",
              had["reserved_qty"] == 200.0 and had["done_qty"] == 0.0
              and had["progress"] == 100.0, str(had))
        done_calls = await js("callsOf('list_set_done')")
        check("5a2 Kein list_set_done mehr über den UI-Flow", done_calls == [], str(done_calls))
        reserved = await js("callsOf('inventory_add_for_list')")
        check("5b Verknüpfte Liste: Reservierung (inventory_add_for_list)",
              reserved != [] and reserved[0]["args"]["listId"] == 2, str(reserved))
        # Backend-Pfad (IPC) bleibt: list_set_done existiert weiterhin für
        # Migrations-Fälle — Status via done+reserved (M9-M).
        await js("inv('list_set_done', { listId: 2, material: 'Hadanite', unit: 'gems', doneQty: 70 })")
        await asyncio.sleep(0.2)
        craft = await js("inv('list_detail', { listId: 1 })") or {"aggregated": []}
        had = [m for m in craft["aggregated"] if m["material"] == "Hadanite"][0]
        check("5c Teilweise 70 schlägt durch", had["done_qty"] == 70.0, str(had))

        # ---- Gate 6: Standalone → inventory_add (keine Reservierung) ----
        await js("selectChip('Solo')")  # Solo-Run
        await asyncio.sleep(0.3)
        check("6a Add-Formular zurück (standalone)", await js("addFormVisible()") is True)
        ok_open = await js("invOpenAt(0)")
        await asyncio.sleep(0.2)
        if ok_open:
            await js("invSet('Depot', '500', null)")
            await asyncio.sleep(0.2)
            await js("invSubmit()")
            await asyncio.sleep(0.5)
        raw_res = await js("callsOf('inventory_add_for_list')") or []
        reserved = [c for c in raw_res if c["args"].get("listId") != 2]
        plain = await js("callsOf('__NOT_RESERVED__')")
        check("6b Standalone: inventory_add OHNE Reservierung",
              reserved == [] and plain != [], f"res={reserved} plain={plain}")
        await shot("m9g-standalone-inv.png")

        # ---- Gate 7: Deutsch ----
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": "window.__M9G_LANG = 'de';"})
        await js("location.reload()")
        await wait_for("document.querySelector('.rail .rentry')", 15)
        await key("F5")
        await asyncio.sleep(0.3)
        await js("clickTab('Listen')")
        await wait_for("listRegion()", 8)
        await js("selectChip('Mine')")
        await asyncio.sleep(0.3)
        hint = await js("lockHint()")
        check("7a DE-Hinweis der verknüpften Liste",
              hint is not None and "Crafting-Liste" in hint and "Craft-Quelle" in hint, str(hint))
        await shot("m9g-de.png")

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


def websockets_connect(ws):
    import websockets
    return websockets.connect(ws, max_size=None)


asyncio.run(main())
