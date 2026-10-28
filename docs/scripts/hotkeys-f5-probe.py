#!/usr/bin/env python3
"""PB t_1c9bc240 probe — Mode-Hotkeys (F11/F3/F4) + F5-Resync, self-contained.

Gates:
  1. Rail zeigt NEUE Defaults: F3 Routen · F4 Inventar · F11 Hangar (Migration)
  2. Suche: KEIN F5-Hinweis, F5-Druck löst KEINEN Call aus
  3. Market/Angebote: F5-Hinweis; F5 → get_marketplace mit force_refresh=true
  4. Routen (F3): F5-Hinweis „Routen-Sweep"; F5 → sync_now
  5. Hangar (F11): Modus aktiv
  6. Settings: „Mode-Hotkeys"-Sektion, Hangar Ändern → F9 übernommen,
     F5-Versuch abgelehnt (reserviert), Rail zeigt F9

Backend = Tauri-Stub (docs/scripts/smoke-stub.js), injected per-CDP.
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_1c9bc240")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS = 0
FAIL = 0


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS {name}")
    else:
        FAIL += 1
        print(f"  FAIL {name}  {detail}")


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
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-hotkeys-f5",
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

        await wrap("""
window.__CALLS__ = [];
const __origInvoke = window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke = function (cmd, args) {
  window.__CALLS__.push(cmd);
  return __origInvoke(cmd, args);
};
window.pressKey = function (key) {
  window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
};
window.footHints = function () {
  const el = document.querySelector('.foot .hints');
  return el ? el.textContent : '';
};
window.railEntry = function (id) {
  const b = [...document.querySelectorAll('.rail .rentry')].find(
    (x) => x.getAttribute('aria-label') || '');
  return [...document.querySelectorAll('.rail .rentry')].map((x) => x.getAttribute('aria-label') || '');
};
window.railActive = function () {
  const on = document.querySelector('.rail .rentry.is-on');
  return on ? on.getAttribute('aria-label') : null;
};
""")

        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        # ---- 1) Rail-Defaults (Migration: Config ohne mode_hotkeys) ----
        labels = await js("railEntry()")
        check("1a Rail: Routen = F3", any("Handelsrouten (F3)" in (l or "") for l in (labels or [])), str(labels))
        check("1b Rail: Inventar = F4", any("Inventar" in (l or "") and "(F4)" in (l or "") for l in (labels or [])))
        check("1c Rail: Hangar = F11", any("Hangar" in (l or "") and "(F11)" in (l or "") for l in (labels or [])))

        # ---- 2) Suche: kein F5-Hinweis, kein Call ----
        hints = await js("footHints()")
        check("2a Suche: kein F5-Hinweis", "F5" not in (hints or ""), hints)
        await js("window.__CALLS__ = []; pressKey('F5')")
        await asyncio.sleep(0.5)
        calls = await js("window.__CALLS__")
        check("2b Suche: F5 ohne Call", calls == [], str(calls))

        # ---- 3) Market/Angebote: F5-Hinweis + force_refresh ----
        await js("pressKey('F2')")
        await wait_for("footHints().includes('F5')", 8)
        hints = await js("footHints()")
        check("3a Market: F5-Hinweis 'Market syncen'", "F5" in (hints or "") and "Market syncen" in (hints or ""), hints)
        await js("pressKey('F5')")
        await wait_for("(document.querySelector('.f5-msg')||{}).textContent", 8)
        msg = await js("(document.querySelector('.f5-msg')||{}).textContent")
        check("3b F5 → 'Market aktualisiert'", (msg or "").startswith("Market aktualisiert"), msg)
        last = await js("(__MKT_CALLS__||[]).slice(-1)[0] || null")
        check("3c get_marketplace mit force_refresh=true", bool(last and last.get("force_refresh") is True), str(last))
        await js("document.querySelector('.pal-root') || null")
        await call("Page.captureScreenshot", {"format": "png"})  # warm-up
        shot = ok(await call("Page.captureScreenshot", {"format": "png"}))
        (OUT / "pb-market-f5-hint.png").write_bytes(__import__("base64").b64decode(shot["data"]))

        # ---- 4) Routen (F3): F5-Hinweis + sync_now ----
        await js("pressKey('F3')")
        await asyncio.sleep(0.4)
        check("4a F3 → Routen aktiv", (await js("railActive()") or "").startswith("Handelsrouten"), await js("railActive()"))
        hints = await js("footHints()")
        check("4b Routen: F5-Hinweis 'Routen-Sweep'", "Routen-Sweep" in (hints or ""), hints)
        await js("window.__CALLS__ = []; pressKey('F5')")
        await asyncio.sleep(0.6)
        calls = await js("window.__CALLS__")
        check("4c F5 in Routen → sync_now", "sync_now" in (calls or []), str(calls))

        # ---- 5) Hangar (F11) ----
        await js("pressKey('F11')")
        await asyncio.sleep(0.4)
        check("5 F11 → Hangar aktiv", (await js("railActive()") or "").startswith("Hangar"), await js("railActive()"))

        # ---- 6) Settings: Mode-Hotkeys konfigurierbar ----
        await js("pressKey('F12')")
        await wait_for("document.body.textContent.includes('Mode-Hotkeys')", 8)
        body = await js("document.body.textContent")
        check("6a Settings: 'Mode-Hotkeys'-Sektion", "Mode-Hotkeys" in (body or ""))
        check("6b Settings: F5-Reserviert-Hinweis", "reserviert" in (body or ""))
        # Hangar-Zeile ändern → F9
        rows = await js("[...document.querySelectorAll('.cfg-sc-row')].map((r) => r.textContent)")
        hangar_row = next((i for i, t in enumerate(rows or []) if "Hangar" in t and "Aktiv" in t), None)
        check("6c Settings: Hangar-Zeile mit Aktiv-Label", hangar_row is not None, str(rows))
        if hangar_row is not None:
            await js(f"document.querySelectorAll('.cfg-sc-row')[{hangar_row}].querySelector('.cfg-sc-capture').click()")
            await js("pressKey('F9')")
            await asyncio.sleep(0.6)
            body = await js("document.body.textContent")
            check("6d Hangar → F9 übernommen (Toast + Aktiv)", "F9" in (body or "") and "übernommen" in (body or ""), (body or "")[-400:])
            await js("pressKey('Escape')")  # Config zu
            await asyncio.sleep(0.4)
            labels = await js("railEntry()")
            check("6e Rail zeigt F9 für Hangar", any("Hangar" in (l or "") and "(F9)" in (l or "") for l in (labels or [])), str(labels))
        # F5-Versuch im Capture
        await js("pressKey('F12')")
        await wait_for("document.body.textContent.includes('Mode-Hotkeys')", 8)
        rows = await js("[...document.querySelectorAll('.cfg-sc-row')].map((r) => r.textContent)")
        hangar_row = next((i for i, t in enumerate(rows or []) if "Hangar" in t and "Aktiv" in t), None)
        await js(f"document.querySelectorAll('.cfg-sc-row')[{hangar_row}].querySelector('.cfg-sc-capture').click()")
        await js("pressKey('F5')")
        await asyncio.sleep(0.5)
        body = await js("document.body.textContent")
        check("6f F5-Versuch abgelehnt (reserviert)", "reserviert" in (body or ""), (body or "")[-300:])
        shot = ok(await call("Page.captureScreenshot", {"format": "png"}))
        (OUT / "pb-settings-mode-hotkeys.png").write_bytes(__import__("base64").b64decode(shot["data"]))

    print(f"\nPROBE: {PASS} PASS / {FAIL} FAIL")
    server.terminate()
    proc.terminate()
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
