#!/usr/bin/env python3
"""M8-K3 probe (t_a8088d06) — Modi-Verbindung Craft→Mining, self-contained.

Gates:
  1. Craft-Detail: Bedarfserz mit oreId ist klickbar (button, Titel-Hint)
  2. Klick → Mining-Modus (F5-Pfad) mit dem Erz vorgewählt: Orte-pro-Erz-
     Detail SOFORT offen (strata_ore_locations mit oreId gerufen)
  3. Craft-Detail bleibt keep-alive erhalten (versteckt, State unangetastet)
  4. Rückweg Esc/Back → zurück ins Craft-Detail, State erhalten (Detail
     weiterhin offen, Mining-Instanz entladen)
  5. F6-Rückweg (Hotkey statt Esc) löst den Jump-Kontext auf (kein Ghost-State)
  6. Erze OHNE oreId bleiben NICHT klickbar (span)

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_a8088d06")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m8k3-mode-link", ignore_errors=True)


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
    ore: { id: ore, name: ore === 'aluminum_raw' ? 'Aluminum' : ore },
    sort: 'rank',
    tierLegend: null,
    results: {
      space: [{ locationId: 'loc-1', location: 'CRU-L1', system: 'Crusader', rankPct: 90, rank: 1, n: 3, concentration: 22.5, tier: 2, tierName: 'Tier 2' }],
      surface: [], cave: [], exploration: []
    },
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
  window.craftMats = () => [...document.querySelectorAll('.crafting .cmat')].map((r) => ({
    name: r.querySelector('.cm-name')?.textContent.trim(),
    isBtn: !!r.querySelector('button.cm-name'),
    title: r.querySelector('button.cm-name')?.getAttribute('title') || ''
  }));
  window.clickOre = () => {
    const b = [...document.querySelectorAll('.crafting button.cm-name')].find((x) => x.textContent.trim() === 'Aluminum');
    if (!b) return false;
    b.click();
    return true;
  };
  window.craftDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.craftTitle = () => document.querySelector('.crafting .cd-title')?.textContent.trim() || '';
  window.craftHidden = () => {
    const el = document.querySelector('.crafting-keep');
    return el ? el.classList.contains('is-hidden') : null;
  };
  window.miningOpen = () => !!document.querySelector('.mining');
  window.miningDetailTitle = () => document.querySelector('.mining .mdetail .md-title')?.textContent.trim() || '';
  window.pressKey = (key) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  };
})();
"""


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
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-m8k3-mode-link",
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

        # ---- Craft-Detail öffnen ----
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("[...document.querySelectorAll('.crafting .crow-main')].find((b)=>b.textContent.includes('Rsi Andromeda')).click()")
        await wait_for("craftDetailOpen()", 8)
        check("0 Craft-Detail offen (Rsi Andromeda)", await js("craftDetailOpen()") is True)
        await shot("m8-k3-craft-detail-aluminum.png")

        # ---- 1) Bedarfserz klickbar ----
        mats = await js("craftMats()")
        check("1a Aluminum (oreId) ist Button mit Hint",
              mats and mats[0]["isBtn"] is True and "Mining" in mats[0]["title"], str(mats))
        check("1b Hadanite (ohne oreId) NICHT klickbar",
              mats and mats[1]["isBtn"] is False, str(mats))

        # ---- 2) Klick → Mining mit Erz vorgewählt ----
        clicked = await js("clickOre()")
        check("2a Klick auf Bedarfserz", clicked is True)
        await wait_for("miningOpen()", 8)
        check("2b Mining-Modus offen", await js("miningOpen()") is True)
        await wait_for("miningDetailTitle() !== ''", 8)
        check("2c Orte-Detail des Sprung-Erzes offen", await js("miningDetailTitle()") == "Aluminum",
              str(await js("miningDetailTitle()")))
        loc_call = await js("(window.__STRATA_CALLS||[]).filter(c=>c.cmd==='strata_ore_locations').pop()")
        check("2d strata_ore_locations mit oreId", loc_call and loc_call.get("args", {}).get("ore") == "aluminum_raw", str(loc_call))
        await shot("m8-k3-mining-detail-same-ore.png")

        # ---- 3) Craft keep-alive ----
        check("3a Craft-Detail versteckt gemountet (keep-alive)", await js("craftHidden()") is True)
        check("3b Craft-Detail-State erhalten", await js("craftDetailOpen()") is True and await js("craftTitle()") == "Rsi Andromeda")

        # ---- 4) Rückweg-Kaskade (t_ef0bcd6a): Esc im Detail schließt ERST das
        #         Detail (Liste bleibt); erst auf Listen-Ebene geht Esc zurück
        #         ins Craft-Detail (State erhalten).
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.4)
        check("4a Esc im Detail: Detail geschlossen, Mining bleibt offen",
              await js("miningOpen()") is True and await js("miningDetailTitle()") == "")
        check("4b Esc im Detail: Craft weiter keep-alive versteckt", await js("craftHidden()") is True)
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.4)
        check("4c Esc auf Liste: Mining entladen, Craft-Detail sichtbar mit State",
              await js("miningOpen()") is False and await js("craftHidden()") is False
              and await js("craftDetailOpen()") is True and await js("craftTitle()") == "Rsi Andromeda")
        await shot("m8-k3-back-to-craft-detail.png")

        # ---- 5) F6-Hotkey-Rückweg löst Jump-Kontext auf ----
        # Erneut springen, dann über F6 (statt Esc) zurück.
        await js("clickOre()")
        await wait_for("miningOpen()", 8)
        check("5a Zweiter Sprung offen", await js("miningOpen()") is True and await js("miningDetailTitle()") == "Aluminum")
        await js("window.pressKey('F6')")
        await asyncio.sleep(0.4)
        check("5b F6 → Craft-Detail, Jump-Kontext aufgelöst",
              await js("miningOpen()") is False and await js("craftHidden()") is False and await js("craftDetailOpen()") is True)
        check("5c Kein Ghost-Jump-State", await js("craftHidden()") is False)

        # ---- 6) Bestehende Kaskaden unverändert ----
        # 6a: Esc im Craft-Detail schließt NUR das Detail (bestehende
        #     Crafting-Esc-Kaskade, M8-K2-Muster) — der Modus bleibt offen.
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        check("6a Esc schließt Craft-Detail, Modus bleibt", await js("craftDetailOpen()") is False and await js("!!document.querySelector('.crafting')") is True)
        # 6b: Esc erneut verlässt den Modus; F5 (aus der Suche) öffnet Mining
        #     OHNE Jump-Detail (kein strata_ore_locations-Aufruf).
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        check("6b Esc verlässt Crafting → Suche", await js("!!document.querySelector('.crafting')") is False)
        await js("window.pressKey('F5')")
        await wait_for("miningOpen()", 8)
        check("6c F5 öffnet Mining ohne Jump-Detail", await js("miningOpen()") is True and await js("miningDetailTitle()") == "")
        locs_before = await js("(window.__STRATA_CALLS||[]).filter(c=>c.cmd==='strata_ore_locations').length")
        check("6d Kein Auto-Detail ohne Jump", locs_before == 2, f"ore_locations calls={locs_before}")
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        check("6e Esc verlässt Mining → Suche", await js("miningOpen()") is False)

    proc.kill()
    server.kill()
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
