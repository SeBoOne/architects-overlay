#!/usr/bin/env python3
"""M9-A probe (t_60af9ec1) — Blueprint-Pool Besitz-Haken + Verfügbarkeits-Filter.

Gates:
  1. F6 öffnet Crafting; blueprint_owned_get wird beim Mount gerufen
  2. Besitz-Haken in der Liste: Klick toggelt (is-on), stopPropagation
     (öffnet KEIN Detail), IPC blueprint_owned_toggle mit Item-ID
  3. Besitz-Haken oben rechts im Detail (cd-own); synchron zur Liste
  4. Verfügbarkeits-Filter (3-Zustands-Chip): alle → verfügbar → nicht
     verfügbar → alle; Filtermenge korrekt (owned / not-owned)
  5. Persistenz-Contract: Stub spiegelt localStorage — Reload zeigt den
     Haken weiterhin an (Restart-fest auf UI-Ebene; echte Persistenz =
     cargo test blueprint_owned_roundtrip_and_toggle + config.toml-Setter)
  6. i18n-Chip-Labels deutsch bei __probeLang=de

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
"""
import asyncio, json, os, pathlib, shutil, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_60af9ec1")))
OUT.mkdir(parents=True, exist_ok=True)
# Probe-Ports-Hotspot (M8-R1-A): feste Ports 8797/9234 abweichen — 8897/9245.
PORT_HTTP, PORT_CDP = 8897, 9245
URL = f"http://127.0.0.1:{PORT_HTTP}/"

PASS = 0
FAIL = 0

shutil.rmtree("/tmp/cdp-m9a-blueprint-pool", ignore_errors=True)


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
  const mk = (i) => ({
    id: 'item-' + i,
    name: (i === 0 ? 'Rsi Andromeda' : 'Item ' + String(i).padStart(3, '0')),
    slug: 'item-' + i,
    category: i % 3 === 0 ? 'armor' : i % 3 === 1 ? 'weapon' : 'component'
  });
  const ITEMS = { items: Array.from({ length: 12 }, (_, i) => mk(i)), count: 12, attribution: 'Data from celd.space / Strata Mining Tools' };
  const CRAFT = {
    item: { id: 'item-0', name: 'Rsi Andromeda', category: 'weapon' },
    craftTime: 960,
    costs: [
      { material: 'Titanium', oreId: 'titanium_raw', quantity: 2.5, unit: 'SCU', minQuality: 3 },
      { material: 'Hadanite', oreId: null, quantity: 500, unit: 'gems', minQuality: null }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };
  /* M9-A Persistenz-Mirror: Besitz-Set in localStorage gespiegelt — der
     Stub initialisiert bei jedem Load daraus (Restart-fest-Contract). */
  let OWNED = null;
  try {
    OWNED = new Set(JSON.parse(window.localStorage.getItem('__probeOwned') || '[]'));
  } catch (e) { OWNED = new Set(); }
  window.__OWNED = OWNED;
  window.__BP_CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_items') return ITEMS;
    if (cmd === 'strata_craft_cost') { window.__BP_CALLS.push({ cmd, args: a }); return CRAFT; }
    if (cmd === 'blueprint_owned_get') {
      window.__BP_CALLS.push({ cmd });
      return [...window.__OWNED];
    }
    if (cmd === 'blueprint_owned_toggle') {
      window.__BP_CALLS.push({ cmd, args: a });
      const id = (a.id || '').trim();
      const nowOwned = !window.__OWNED.has(id);
      if (nowOwned) window.__OWNED.add(id); else window.__OWNED.delete(id);
      try { window.localStorage.setItem('__probeOwned', JSON.stringify([...window.__OWNED])); } catch (e) {}
      return nowOwned;
    }
    return orig(cmd, args);
  };
  window.bpRows = () => [...document.querySelectorAll('.crafting .crow')].map((r) => ({
    name: r.querySelector('.crow-name')?.textContent.trim(),
    own: r.querySelector('.crow-own')?.classList.contains('is-on')
  }));
  window.bpOwnChip = () => [...document.querySelectorAll('.crafting .cchip-own')].map((b) => b.textContent.trim()).pop();
  window.bpClickOwn = (name) => {
    const row = [...document.querySelectorAll('.crafting .crow')].find((r) => r.querySelector('.crow-name')?.textContent.trim() === name);
    if (!row) return false;
    row.querySelector('.crow-own')?.click();
    return true;
  };
  window.bpCycleFilter = () => { [...document.querySelectorAll('.crafting .cchip-own')].pop()?.click(); };
  window.bpDetailOwn = () => document.querySelector('.crafting .cd-own')?.classList.contains('is-on') || false;
  window.bpClickDetailOwn = () => { document.querySelector('.crafting .cd-own')?.click(); return true; };
  window.bpDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.pressKey = (key) => { window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true })); };
})();
"""


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT_HTTP), "--bind", "127.0.0.1",
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
        f"--remote-debugging-port={PORT_CDP}", "--user-data-dir=/tmp/cdp-m9a-blueprint-pool",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{PORT_CDP}/json/list", timeout=1) as r:
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

        async def bp_calls(cmd=None):
            flt = f".filter((c) => c.cmd === '{cmd}')" if cmd else ""
            return await js(f"window.__BP_CALLS{flt}")

        await wrap(WRAP)
        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        # ---- 1) F6 öffnet Crafting; blueprint_owned_get gerufen ----
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        check("1a F6 öffnet Crafting-Modus", await js("!!document.querySelector('.crafting')") is True)
        await wait_for("(window.__BP_CALLS||[]).some((c) => c.cmd === 'blueprint_owned_get')", 8)
        check("1b blueprint_owned_get beim Mount gerufen", (await bp_calls("blueprint_owned_get") or []) != [])

        rows0 = await js("bpRows()")
        check("1c Liste gerendert (12 Items, alle ohne Haken)", (rows0 or []) and len(rows0) == 12 and all(not r["own"] for r in rows0), str(rows0))

        # ---- 2) Besitz-Haken in der Liste ----
        check("2a Klick auf Haken öffnet KEIN Detail", await js("bpClickOwn('Item 004')") is True and await js("bpDetailOpen()") is False)
        await asyncio.sleep(0.4)
        check("2b blueprint_owned_toggle mit Item-ID", ((await bp_calls("blueprint_owned_toggle") or [{}])[-1] or {}).get("args", {}).get("id") == "item-4",
              str(await bp_calls("blueprint_owned_toggle")))
        rows1 = await js("bpRows()")
        own004 = next((r for r in rows1 if r["name"] == "Item 004"), None)
        check("2c Haken is-on nach Klick (alphab. Sortierung)", rows1 and own004 and own004["own"] is True and sum(1 for r in rows1 if r["own"]) == 1, str(rows1))
        await js("bpClickOwn('Item 004')")
        await asyncio.sleep(0.4)
        rows2 = await js("bpRows()")
        check("2d Zweiter Klick entfernt den Haken", rows2 and all(not r["own"] for r in rows2), str(rows2))

        # ---- 3) Detail-Haken, synchron zur Liste ----
        await js("[...document.querySelectorAll('.crafting .crow-main')].find((b) => b.textContent.includes('Rsi Andromeda')).click()")
        await wait_for("bpDetailOpen()", 8)
        check("3a Detail offen", await js("bpDetailOpen()") is True)
        check("3b Detail-Haken oben rechts (cd-own)", await js("!!document.querySelector('.crafting .cd-own')") is True)
        await js("bpClickDetailOwn()")
        await asyncio.sleep(0.4)
        check("3c Detail-Haken is-on", await js("bpDetailOwn()") is True)
        await shot("m9a-detail-own.png")
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.4)
        rows3 = await js("bpRows()")
        rsi = next((r for r in rows3 if r["name"] == "Rsi Andromeda"), None)
        check("3d Listen-Haken synchron (Rsi Andromeda)", rows3 and rsi and rsi["own"] is True and sum(1 for r in rows3 if r["own"]) == 1, str(rows3))
        await shot("m9a-owned-list-detail.png")

        # ---- 4) Verfügbarkeits-Filter ----
        check("4a Chip-Start = alle", (await js("bpOwnChip()") or "").lower().find("all") >= 0, str(await js("bpOwnChip()")))
        await js("bpCycleFilter()")
        await asyncio.sleep(0.4)
        rows4 = await js("bpRows()")
        check("4b verfügbar: nur Besitz-Markierte (Rsi Andromeda)",
              (await js("bpOwnChip()") or "").lower().find("owned") >= 0 and len(rows4) == 1 and rows4[0]["name"] == "Rsi Andromeda" and rows4[0]["own"] is True, str(rows4))
        await js("bpCycleFilter()")
        await asyncio.sleep(0.4)
        rows5 = await js("bpRows()")
        check("4c nicht verfügbar: alle außer Besitz-Markierte",
              (await js("bpOwnChip()") or "").lower().find("not owned") >= 0 and len(rows5) == 11 and all(r["name"] != "Rsi Andromeda" for r in rows5), str(len(rows5)))
        await js("bpCycleFilter()")
        await asyncio.sleep(0.4)
        check("4d zurück auf alle (12 Zeilen)", len(await js("bpRows()") or []) == 12, str(await js("bpRows()")))
        await shot("m9a-own-filter.png")

        # ---- 5) Persistenz-Contract (UI-Ebene): Reload ----
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await asyncio.sleep(0.5)
        rows6 = await js("bpRows()")
        rsi6 = next((r for r in rows6 if r["name"] == "Rsi Andromeda"), None)
        check("5 Reload: Haken weiterhin gesetzt (localStorage-Mirror)",
              rows6 and rsi6 and rsi6["own"] is True and sum(1 for r in rows6 if r["own"]) == 1, str(rows6))

        # ---- 6) Deutsch-Labels (i18n) ----
        await js("try { localStorage.setItem('__probeLang', 'de'); } catch (e) {}")
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await asyncio.sleep(0.5)
        chip = await js("bpOwnChip()")
        check("6a Deutscher Chip-Label", chip == "Verfügbarkeit: alle", str(chip))
        row_own_title = await js("document.querySelector('.crafting .crow-own')?.title || document.querySelector('.crafting .crow-own')?.getAttribute('title')")
        check("6b Deutscher Haken-Title", row_own_title == "Blueprint als im Besitz markieren", str(row_own_title))
        await shot("m9a-owned-de.png")

        # Cleanup Probe-Spiegel (kein Persistenz-Zwang ins nächste Run)
        await js("try { localStorage.removeItem('__probeOwned'); localStorage.removeItem('__probeLang'); } catch (e) {}")

    proc.kill()
    server.kill()
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
