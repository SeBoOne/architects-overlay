#!/usr/bin/env python3
"""M8-K3 Stick-Probe (t_ef0bcd6a) — Klebe-Effekt + Back/Esc-Kaskade im Sprung-Kontext.

Repo-Probe (aus der Reviewer-Ad-hoc-Probe übernommen, t_a8088d06/t_ef0bcd6a).

Frage: Nach dem Craft→Mining-Sprung (Aluminum vorgewählt, Detail offen):
  A) Klick auf ein ANDERES Erz (Titanium) — bleibt Titanium offen oder
     reißt der $effect das Detail sofort zurück auf Aluminum?
  B) Back-Button im Erz-Detail — schließt das Detail (Mining bleibt,
     Liste sichtbar) oder führt er direkt zurück ins Crafting?
  C) Esc auf Listen-Ebene im Sprung-Kontext (wenn A/B einen Weg dorthin
     erlauben) — normales Mining-Ende oder Sprung-Rückweg?

Isolierter Boot: eigener Port 9235 + eigenes user-data-dir.
"""
import asyncio, json, os, pathlib, shutil, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_ef0bcd6a")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8798/"

PASS, FAIL = 0, 0
shutil.rmtree("/tmp/cdp-m8k3-stick", ignore_errors=True)


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
  const ITEMS = { items: [
    { id: 'item-0', name: 'Rsi Andromeda', slug: 'rsi-andromeda', category: 'weapon' },
    { id: 'item-1', name: 'Item 001', slug: 'item-001', category: 'armor' }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };
  const CRAFT = {
    item: { id: 'item-0', name: 'Rsi Andromeda', category: 'weapon' },
    craftTime: 960,
    costs: [
      { material: 'Aluminum', oreId: 'aluminum_raw', quantity: 2.5, unit: 'SCU', minQuality: 3 },
      { material: 'Hadanite', oreId: null, quantity: 500, unit: 'gems', minQuality: null }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  const ORES = { ores: [
    { id: 'aluminum_raw', name: 'Aluminum', slug: 'aluminum', category: 'ship', tier: 'common', scanSignature: 0.42, instability: 0.2, resistance: 0.1 },
    { id: 'titanium_raw', name: 'Titanium', slug: 'titanium', category: 'ship', tier: 'uncommon', scanSignature: 0.31, instability: 0.3, resistance: 0.2 }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };
  const LOCS = { locations: [
    { id: 'loc-1', name: 'CRU-L1', slug: 'cru-l1', system: 'Crusader', type: 'space_station' }
  ], count: 1, attribution: 'Data from celd.space / Strata Mining Tools' };
  const ORE_LOCS = (ore) => ({
    ore: { id: ore, name: ore === 'aluminum_raw' ? 'Aluminum' : (ore === 'titanium_raw' ? 'Titanium' : ore) },
    sort: 'rank', tierLegend: null,
    results: { space: [{ locationId: 'loc-1', location: 'CRU-L1', system: 'Crusader', rankPct: 90, rank: 1, n: 3, concentration: 22.5, tier: 2, tierName: 'Tier 2' }], surface: [], cave: [], exploration: [] },
    attribution: 'Data from celd.space / Strata Mining Tools'
  });
  window.__STRATA_CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_items') { window.__STRATA_CALLS.push({ cmd }); return ITEMS; }
    if (cmd === 'strata_craft_cost') { window.__STRATA_CALLS.push({ cmd, args: a }); return CRAFT; }
    if (cmd === 'strata_ores') { window.__STRATA_CALLS.push({ cmd }); return ORES; }
    if (cmd === 'strata_locations') { window.__STRATA_CALLS.push({ cmd }); return LOCS; }
    if (cmd === 'strata_ore_locations') { window.__STRATA_CALLS.push({ cmd, args: a }); return ORE_LOCS(a.ore); }
    return orig(cmd, args);
  };
  window.pressKey = (key) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  };
  window.craftDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.clickCraftOre = () => {
    const b = [...document.querySelectorAll('.crafting button.cm-name')].find((x) => x.textContent.trim() === 'Aluminum');
    if (!b) return false;
    b.click();
    return true;
  };
  window.miningDetailTitle = () => document.querySelector('.mining .mdetail .md-title')?.textContent.trim() || '';
  window.miningOpen = () => !!document.querySelector('.mining');
  window.craftHidden = () => {
    const el = document.querySelector('.crafting-keep');
    return el ? el.classList.contains('is-hidden') : null;
  };
  window.oreRows = () => [...document.querySelectorAll('.mining .mrow-main')].map((b) => b.querySelector('.mrow-name')?.textContent.trim() || '');
  window.clickOreRow = (name) => {
    const b = [...document.querySelectorAll('.mining .mrow-main')].find((x) => x.querySelector('.mrow-name')?.textContent.trim() === name);
    if (!b) return false;
    b.click();
    return true;
  };
  window.backBtn = () => !!document.querySelector('.mining .mback');
  window.clickBack = () => {
    const b = document.querySelector('.mining .mback');
    if (!b) return false;
    b.click();
    return true;
  };
})();
"""


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8798", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9235", "--user-data-dir=/tmp/cdp-m8k3-stick",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9235/json/list", timeout=1) as r:
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

        await wrap(WRAP)
        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        # Setup: F6 → Crafting → Detail → Aluminum klicken → Mining-Sprung
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("[...document.querySelectorAll('.crafting .crow-main')].find((b)=>b.textContent.includes('Rsi Andromeda')).click()")
        await wait_for("craftDetailOpen()", 8)
        await js("clickCraftOre()")
        await wait_for("miningOpen() && miningDetailTitle() !== ''", 8)
        check("setup Sprung: Mining offen, Detail=Aluminum",
              await js("miningDetailTitle()") == "Aluminum", str(await js("miningDetailTitle()")))

        # ---- A) Anderes Erz öffnen: Titanium — reißt es zurück? ----
        rows = await js("oreRows()")
        check("A1 Erzliste enthält beide Erze", rows == ["Aluminum", "Titanium"], str(rows))
        await js("clickOreRow('Titanium')")
        await asyncio.sleep(0.6)  # Effect-Re-Run-Zeit
        title_after_ti = await js("miningDetailTitle()")
        check("A2 Titanium bleibt offen nach Klick", title_after_ti == "Titanium", f"Detail={title_after_ti!r}")
        ore_calls = await js("(window.__STRATA_CALLS||[]).filter(c=>c.cmd==='strata_ore_locations').map(c=>c.args.ore)")
        check("A3 strata_ore_locations nur je Erz einmal (kein Re-Call Aluminum)", 
              ore_calls == ["aluminum_raw", "titanium_raw"], str(ore_calls))

        # ---- B) Back-Button im Detail: schließt Detail (Mining bleibt) oder Sprung-Rückweg? ----
        await js("clickBack()")
        await asyncio.sleep(0.6)
        mining_still = await js("miningOpen()")
        title_after_back = await js("miningDetailTitle()")
        hidden = await js("craftHidden()")
        check("B1 Back im Detail: Mining bleibt gemountet", mining_still is True, f"miningOpen={mining_still}")
        check("B2 Back im Detail: Detail geschlossen (Titel leer)", title_after_back == "", f"title={title_after_back!r}")
        check("B3 Back im Detail: Crafting weiter versteckt", hidden is True, f"craftHidden={hidden}")

        # ---- C) Jetzt Esc auf Listen-Ebene (kein Detail): normal Mining-Ende oder Sprung-Rückweg? ----
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.5)
        mining_after_esc = await js("miningOpen()")
        hidden_after_esc = await js("craftHidden()")
        check("C1 Esc auf Listen-Ebene: Mining geschlossen", mining_after_esc is False, f"miningOpen={mining_after_esc}")
        check("C2 Esc: Crafting wieder sichtbar (Sprung-Rückweg statt Suche)", hidden_after_esc is False, f"craftHidden={hidden_after_esc}")

    proc.kill()
    server.kill()
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
