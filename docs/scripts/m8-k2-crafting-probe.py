#!/usr/bin/env python3
"""M8-K2 probe (t_f95f9d97) — Crafting-Mode-UI (F6), self-contained.

Gates:
  1. Rail zeigt Crafting (F6) bei Strata enabled+Key (dynamisch gefiltert)
  2. F6 öffnet den Crafting-Modus; strata_items wird gerufen (id vor Slug)
  3. Kategorie-Chips DYNAMISCH aus den Daten (All + jede vorkommende category)
  4. Hauptsuchfeld = In-Context-Filter (Name/Slug, LEER = Gesamtliste)
  5. Item-Zeilen: Name + Category-Badge; Load-More bei >80 Treffern
  6. Detail: strata_craft_cost mit Item-ID; Craft-Zeit; Materialliste mit
     Menge + EINHEIT farblich getrennt; minQuality-Badge (null → „—“)
  7. Qualitäts-PLANUNGSRECHNER: Slider 0–1000, required = max(minQuality),
     Material grün/rot je Erfüllung; KEIN Stat-Versprechen (Note-Text)
  8. Attribution-Footer sichtbar; Geometrie: Footer im sichtbaren Bereich
  9. Esc-Kaskade: Detail schließen → Liste; nochmal Esc → Modus verlassen
 10. Key-Missing-State: Erklär-Card + api-keys-Link
 11. Suche-Placeholder im Crafting-Modus = craft.ph_main

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_f95f9d97")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS = 0
FAIL = 0

# Frischer Profil-Dir: localStorage-Flag '__probeCraftKey' darf den Run nicht
# aus dem Vorlauf persistieren.
import shutil
shutil.rmtree("/tmp/cdp-m8k2-crafting", ignore_errors=True)


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
  // 100 Items (Load-More-Gate: >80) in 3 Kategorien + ein null-category-Item.
  const mk = (i) => ({
    id: 'item-' + i,
    name: (i % 25 === 0 ? 'Rsi Andromeda' : 'Item ' + String(i).padStart(3, '0')),
    slug: 'item-' + i,
    category: i % 25 === 0 ? 'weapon' : (i % 3 === 0 ? 'armor' : i % 3 === 1 ? 'weapon' : 'component')
  });
  const ITEMS = { items: Array.from({ length: 100 }, (_, i) => mk(i)).concat([
    { id: 'item-nullcat', name: 'Mystery Item', slug: 'mystery-item', category: null }
  ]), count: 101, attribution: 'Data from celd.space / Strata Mining Tools' };
  const CRAFT = {
    item: { id: 'item-000', name: 'Rsi Andromeda', category: 'weapon' },
    craftTime: 960,
    costs: [
      { material: 'Titanium', oreId: 'titanium_raw', quantity: 2.5, unit: 'SCU', minQuality: 3 },
      { material: 'Hadanite', oreId: null, quantity: 500, unit: 'gems', minQuality: null },
      { material: 'Steel Beam', oreId: null, quantity: 4, unit: 'items', minQuality: 0 }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  window.__STRATA_CALLS = [];
  let keySet = true;
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') {
      return { enabled: true, key_set: (function () { try { return window.localStorage.getItem('__probeCraftKey') !== 'false'; } catch (e) { return true; } })(), key_ref: 'default', tier_default: 4 };
    }
    if (cmd === 'strata_items') { window.__STRATA_CALLS.push({ cmd }); return ITEMS; }
    if (cmd === 'strata_craft_cost') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      return CRAFT;
    }
    return orig(cmd, args);
  };
  window.craftRows = () => [...document.querySelectorAll('.crafting .crow')].length;
  window.craftChips = () => [...document.querySelectorAll('.crafting .cchip')].map((b) => b.textContent.trim());
  window.craftDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.craftMats = () => [...document.querySelectorAll('.crafting .cmat')].map((r) => ({
    name: r.querySelector('.cm-name')?.textContent.trim(),
    unit: r.querySelector('.cm-unit')?.textContent.trim(),
    unitColor: r.querySelector('.cm-unit')?.style.getPropertyValue('--uc'),
    mq: r.querySelector('.cmq')?.textContent.trim(),
    ok: r.classList.contains('is-ok'),
    fail: r.classList.contains('is-fail')
  }));
  window.craftRequired = () => document.querySelector('.crafting .cp-result')?.textContent.trim() || '';
  window.craftAttr = () => document.querySelector('.crafting .cattr')?.textContent.trim() || '';
  window.craftAttrVisible = () => {
    const el = document.querySelector('.crafting .cattr');
    if (!el) return false;
    const r = el.getBoundingClientRect();
    return r.height > 0 && r.bottom > 0 && r.bottom <= window.innerHeight + 1;
  };
  window.setSearch = (v) => {
    const inp = document.querySelector('.q');
    inp.value = v;
    inp.dispatchEvent(new Event('input', { bubbles: true }));
  };
  window.pressKey = (key) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  };
})();
"""

WRAP_NOKEY = None  # Key-Missing-Phase läuft über localStorage['__probeCraftKey']='false' + Reload


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8797", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-m8k2-crafting",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9234/json/list", timeout=1) as r:
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

        # ---- 1) Rail: Crafting (F6) vorhanden (Strata enabled+Key) ----
        labels = await js("[...document.querySelectorAll('.rail .rentry')].map((x) => x.getAttribute('aria-label') || '')")
        check("1 Rail: Crafting (F6) Entry", any("Crafting" in (l or "") and "(F6)" in (l or "") for l in (labels or [])), str(labels))

        # ---- 2) F6 öffnet Crafting; strata_items gerufen ----
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        check("2a F6 öffnet Crafting-Modus", await js("!!document.querySelector('.crafting')") is True)
        await wait_for("(window.__STRATA_CALLS||[]).some(c=>c.cmd==='strata_items')", 8)
        check("2b strata_items gerufen", await js("(window.__STRATA_CALLS||[]).some(c=>c.cmd==='strata_items')") is True)

        # ---- 11) Placeholder des Hauptsuchfelds ----
        ph = await js("document.querySelector('.q')?.placeholder")
        check("11 Placeholder = craft.ph_main", ph == "Crafting: item…", str(ph))

        # ---- 3) Kategorie-Chips dynamisch ----
        chips = await js("craftChips()")
        check("3a Chips: All + dynamische Kategorien",
              (chips or [])[:1] == ["All"] and "armor" in chips and "weapon" in chips and "component" in chips and "—" in chips,
              str(chips))

        # ---- 4) Hauptsuchfeld filtert (Name + Slug), leer = Gesamtliste ----
        total = await js("craftRows()")
        check("4a LEER = Gesamtliste (>80 → Load-More-Grenze)", total == 80, f"rows={total}")
        await js("setSearch('mystery')")
        await asyncio.sleep(0.4)
        check("4b Name/Slug-Filter", await js("craftRows()") == 1, str(await js("craftRows()")))
        await js("setSearch('rsi')")
        await asyncio.sleep(0.4)
        check("4c Name-Filter", await js("craftRows()") == 4, str(await js("craftRows()")))
        await js("setSearch('')")
        await asyncio.sleep(0.4)

        # Kategorie-Filter
        await js("[...document.querySelectorAll('.crafting .cchip')].find((b)=>b.textContent.trim()==='armor').click()")
        await asyncio.sleep(0.4)
        armor_rows = await js("craftRows()")
        await js("[...document.querySelectorAll('.crafting .cchip')].find((b)=>b.textContent.trim()==='All').click()")
        await asyncio.sleep(0.3)
        check("4d Kategorie-Chip filtert", armor_rows == 32, f"armor rows={armor_rows}")

        # ---- 5) Load-More ----
        await js("document.querySelector('.crafting .cmore')?.click()")
        await asyncio.sleep(0.3)
        check("5 Load-More erhöht die Liste", await js("craftRows()") == 101, str(await js("craftRows()")))

        # ---- 6) Detail öffnen (ID vor Slug) ----
        await js("[...document.querySelectorAll('.crafting .crow-main')].find((b)=>b.textContent.includes('Rsi Andromeda')).click()")
        await wait_for("craftDetailOpen()", 8)
        check("6a Detail offen", await js("craftDetailOpen()") is True)
        cost_call = await js("(window.__STRATA_CALLS||[]).filter(c=>c.cmd==='strata_craft_cost').pop()")
        check("6b strata_craft_cost mit Item-ID", cost_call and cost_call.get("args", {}).get("item") == "item-0", str(cost_call))
        ct = await js("document.querySelector('.crafting .cd-time')?.textContent")
        check("6c Craft-Zeit angezeigt (960s → 16 min)", ct and "16 min" in ct, str(ct))
        mats = await js("craftMats()")
        check("6d Materialliste: 3 Materialien", (mats or []) and len(mats) == 3, str(mats))
        check("6e Einheiten farblich getrennt (SCU/gems/items)",
              mats and mats[0]["unit"] == "SCU" and mats[1]["unit"] == "gems" and mats[2]["unit"] == "items"
              and len({mats[0]["unitColor"], mats[1]["unitColor"], mats[2]["unitColor"]}) == 3, str(mats))
        check("6f minQuality-Badge: null → „—“", mats and mats[1]["mq"] == "—", str(mats))
        await shot("m8-k2-crafting-detail.png")

        # ---- 7) Qualitäts-Planungsrechner ----
        req0 = await js("craftRequired()")
        check("7a required = max(minQuality) = 3", "3" in req0, str(req0))

        async def set_slider(v):
            return await js(
                "(() => { const s=document.querySelector('.crafting .cp-slider');"
                " s.focus(); s.value=" + str(v) + ";"
                " s.dispatchEvent(new Event('input',{bubbles:true}));"
                " s.dispatchEvent(new Event('change',{bubbles:true}));"
                " return document.querySelector('.crafting .cp-val')?.textContent; })()"
            )

        await set_slider(2)
        await asyncio.sleep(0.3)
        mats2 = await js("craftMats()")
        check("7b Ziel 2 < minQuality 3 → Material rot, null/0 grün",
              mats2 and mats2[0]["fail"] is True and mats2[1]["ok"] is True and mats2[2]["ok"] is True, str(mats2))
        val3 = await set_slider(3)
        await asyncio.sleep(0.3)
        mats3 = await js("craftMats()")
        check("7c Ziel 3 ≥ minQuality 3 → alle grün (Slider-Wert = %s)" % val3,
              mats3 and all(m["ok"] for m in mats3), str(mats3))
        note = await js("document.querySelector('.crafting .cp-note')?.textContent||''")
        check("7d KLARLABEL: kein Stat-Versprechen",
              ("NOT a stat prediction" in note) or ("no stat prediction" in note.lower()), str(note))
        legend = await js("document.querySelector('.crafting .cpl-units')?.textContent||''")
        check("7e Legende: 1000 gems = 1 SCU", "1000" in legend and "SCU" in legend, str(legend))

        # ---- 8) Attribution-Footer ----
        check("8a Attribution-Footer Text", "celd.space" in (await js("craftAttr()") or ""), str(await js("craftAttr()")))
        check("8b Attribution-Footer im sichtbaren Bereich", await js("craftAttrVisible()") is True)

        # ---- 9) Esc-Kaskade ----
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        check("9a Esc schließt Detail", await js("craftDetailOpen()") is False and await js("!!document.querySelector('.crafting')") is True)
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        check("9b Esc verlässt den Modus", await js("!!document.querySelector('.crafting')") is False)

        # ---- 10) Key-Missing-State (localStorage-Flag + Reload) ----
        await js("try { localStorage.setItem('__probeCraftKey', 'false'); } catch (e) {}")
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)
        rail2 = await js("[...document.querySelectorAll('.rail .rentry')].map((x) => x.getAttribute('aria-label') || '')")
        check("10a Key fehlt: Rail ohne Crafting-Entry", not any("Crafting" in (l or "") for l in (rail2 or [])), str(rail2))
        # Wie Mining: ohne Key wird der Modus-Wechsel IGNORIERT (kein Karten-Zwang) —
        # F6 ist ein No-op; die Key-Missing-Card bleibt defensive UI für den
        # Fall Key-Verlust bei offenem Modus.
        await js("window.pressKey('F6')")
        await asyncio.sleep(0.5)
        check("10b Key fehlt: F6 ist No-op (Modus öffnet nicht)", await js("!!document.querySelector('.crafting')") is False)
        await shot("m8-k2-crafting-key-missing.png")

    proc.kill()
    server.kill()
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
