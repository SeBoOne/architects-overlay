#!/usr/bin/env python3
"""Kurzer Centering-Check für t_5fe8eb4c: .shell/.pal-root-Rects im Probe-Stub."""
import asyncio, json, urllib.request
import websockets

ROOT = "/home/sebo/.hermes/kanban/boards/architects-overlay/workspaces/t_5fe8eb4c/repo"
STUB = open(ROOT + "/docs/scripts/smoke-stub.js").read()
URL = "http://127.0.0.1:8799/"

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8799", "--bind", "127.0.0.1",
        "--directory", ROOT + "/build",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    for _ in range(40):
        try:
            urllib.request.urlopen(URL, timeout=1); break
        except Exception:
            await asyncio.sleep(0.25)
    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9236", "--user-data-dir=/tmp/cdp-center3",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            pages = [t for t in json.load(urllib.request.urlopen("http://127.0.0.1:9236/json/list", timeout=1)) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception:
            await asyncio.sleep(0.25)
    async with websockets.connect(ws, max_size=None) as cdp:
        mid = [0]; pend = {}
        async def call(m, p=None):
            mid[0] += 1; f = asyncio.get_event_loop().create_future(); pend[mid[0]] = f
            await cdp.send(json.dumps({"id": mid[0], "method": m, "params": p or {}}))
            return await f
        async def rl():
            async for raw in cdp:
                m = json.loads(raw)
                if "id" in m and m["id"] in pend:
                    pend[m["id"]].set_result(m)
        asyncio.create_task(rl())
        async def js(e):
            r = (await call("Runtime.evaluate", {"expression": e, "returnByValue": True, "awaitPromise": True}))
            r = r.get("result", {}).get("result", {})
            if r.get("exceptionDetails"):
                return "EXC:" + json.dumps(r["exceptionDetails"])[:300]
            return r.get("value")
        await call("Page.enable")
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(3)
        print("win:", await js("document.documentElement.dataset.window || 'unset'"))
        print("shell:", await js("(function(){var s=document.querySelector('.shell'); return s? JSON.stringify(s.getBoundingClientRect()) : 'none'})()"))
        print("palroot:", await js("(function(){var s=document.querySelector('.pal-root'); return s? JSON.stringify(s.getBoundingClientRect()) : 'none'})()"))
        print("bodycss:", await js("(function(){var b=getComputedStyle(document.body),p=getComputedStyle(document.querySelector('.pal-root')); return JSON.stringify({bodyM:b.marginLeft,bodyP:b.paddingLeft,palM:p.marginLeft,palP:p.paddingLeft})})()"))
    server.terminate(); proc.terminate()

asyncio.run(main())
