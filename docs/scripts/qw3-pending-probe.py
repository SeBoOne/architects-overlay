#!/usr/bin/env python3
"""QW-3 Mini-Probe (Deploy t_b149e29e) — Hinweis-UI im Mein-Shop.

Lädt die deployte UI (build/, Chunk 2.DBMJS8yz) mit leeren eigenen
Listings (Stub: kein Secret-Key-Flow, GET liefert keine eigenen Offers)
und prüft, dass der QW-3-Hinweis-Text erscheint:
  'Keine eigenen aktiven Listings. Wartet ein Listing auf Freigabe ...'
"""
import asyncio, json, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
PORT = int(__import__("os").environ.get("QW3_PORT", "8813"))
CDP_PORT = int(__import__("os").environ.get("QW3_CDP_PORT", "9245"))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
        "--directory", str(ROOT / "build"),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    for _ in range(60):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/", timeout=1) as r:
                if r.status == 200: break
        except Exception: pass
        await asyncio.sleep(0.25)

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-qw3",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{CDP_PORT}/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception: pass
        await asyncio.sleep(0.25)

    async with websockets.connect(ws, max_size=None) as cdp:
        mid = 0; pending = {}
        async def call(m, p=None):
            nonlocal mid
            mid += 1
            fut = asyncio.get_event_loop().create_future()
            pending[mid] = fut
            await cdp.send(json.dumps({"id": mid, "method": m, "params": p or {}}))
            return await fut
        async def recv():
            async for raw in cdp:
                m = json.loads(raw)
                if "id" in m and m["id"] in pending:
                    pending[m["id"]].set_result(m)
        asyncio.create_task(recv())
        def ok(r):
            if "error" in r: raise RuntimeError(r["error"])
            return r.get("result", {})
        await call("Page.enable")
        EXTRA = (
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "window.__stubState.uex_username = 'sebo_one';"
            "window.__SHOP_OFFERS__ = [];"
            "window.__SHOP_NEGOTIATIONS__ = [];"
            "window.__SHOP_TRADES__ = [];"
        )
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": EXTRA})
        await call("Page.navigate", {"url": f"http://127.0.0.1:{PORT}/"})
        await asyncio.sleep(3.0)
        async def js(expr):
            r = await call("Runtime.evaluate", {"expression": expr, "returnByValue": True})
            if "error" in r: raise RuntimeError(r["error"])
            return r.get("result", {}).get("result", {}).get("value")
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        for k, vk, code in [("F2", 113, "F2"), ("F2", 113, "F2")]:
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.5)
        print("tabs:", json.dumps(await js("[...document.querySelectorAll('.mtabs .chip')].map(b=>b.textContent.trim())")))
        r_click = await js("(function(){var b=[...document.querySelectorAll('.chip')].find(x=>x.textContent.trim()==='Mein Shop'); if(b){b.click(); return 'ok';} return 'fehlt';})()")
        print("shop-click:", r_click)
        await asyncio.sleep(1.5)
        js2 = """(() => {
          const t = document.body.innerText;
          const marker = 'Keine eigenen aktiven Listings';
          const idx = t.indexOf(marker);
          return {
            markerFount: idx >= 0,
            snippet: idx >= 0 ? t.slice(idx, idx + 130) : null,
            shopVisible: /MEIN SHOP/.test(t)
          };
        })()"""
        r = await call("Runtime.evaluate", {"expression": js2, "returnByValue": True})
        if "error" in r: raise RuntimeError(r["error"])
        v = r.get("result", {}).get("result", {}).get("value", {})
        print("result:", json.dumps(v))
        sect = await js("(function(){var p=document.querySelector('.pane-market'); return p?p.innerText.slice(0,900):'kein pane';})()")
        print("pane:", sect)
        shot = await call("Page.captureScreenshot", {"format": "png"})
        if "error" not in shot:
            import base64
            out = ROOT / "docs/screens/qw-fixes" / "qw3-pending-listings-hinweis.png"
            out.write_bytes(base64.b64decode(shot["result"]["data"]))
            print("screenshot:", out)
        print("QW3-PROBE:", "PASS" if v.get("markerFount") else "FAIL")
    server.terminate(); proc.terminate()

asyncio.run(main())
