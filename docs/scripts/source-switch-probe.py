#!/usr/bin/env python3
"""Verify t_c3d46cff Punkt 2a: Config-View hat ein live wirkendes Preis-Quelle
(A/B)-Bedienelement — UEX-Chip klicken -> is-on wechselt (und zurück)."""
import asyncio, base64, json, os, pathlib, subprocess, time, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", "/home/sebo/Projects/Architects Overlay"))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screenshots"
OUT.mkdir(parents=True, exist_ok=True)

async def main():
    subprocess.run(["pkill", "-f", "http.server 8795"], stderr=subprocess.DEVNULL)
    srv = subprocess.Popen(
        ["python3", "-m", "http.server", "8795", "--bind", "127.0.0.1", "--directory", str(ROOT / "build")],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(40):
        try:
            urllib.request.urlopen("http://127.0.0.1:8795/", timeout=1); break
        except Exception: time.sleep(0.25)

    subprocess.run(["pkill", "-f", "remote-debugging-port=9225"], stderr=subprocess.DEVNULL)
    chr = subprocess.Popen(
        ["/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
         "--window-size=1920,1080", "--remote-debugging-port=9225", "--user-data-dir=/tmp/cdp-srcsw",
         "about:blank"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ws = None
    for _ in range(60):
        try:
            pages = [t for t in json.load(urllib.request.urlopen("http://127.0.0.1:9225/json/list", timeout=1)) if t.get("type") == "page"]
            if pages: ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception: time.sleep(0.25)
    if not ws:
        srv.terminate(); chr.terminate(); raise SystemExit("no CDP target")

    async with websockets.connect(ws, max_size=None) as cdp:
        mid = {"n": 0}; pend = {}
        async def c2(m, p=None):
            mid["n"] += 1; i = mid["n"]; fut = asyncio.get_event_loop().create_future(); pend[i] = fut
            await cdp.send(json.dumps({"id": i, "method": m, "params": p or {}}))
            return await fut
        async def recv():
            async for raw in cdp:
                m = json.loads(raw)
                if "id" in m and m["id"] in pend: pend[m["id"]].set_result(m)
        rt = asyncio.get_event_loop().create_task(recv())
        await c2("Page.enable")
        await c2("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await c2("Page.navigate", {"url": "http://127.0.0.1:8795/"})
        await asyncio.sleep(2.5)
        async def ev(expr):
            r = await c2("Runtime.evaluate", {"expression": expr, "returnByValue": True})
            return r["result"]["result"].get("value")
        # Config öffnen (Zahnrad)
        await ev("document.querySelector('button.gear')?.click()")
        await asyncio.sleep(1.0)
        title = await ev("document.querySelector('.cfg-title')?.innerText||''")
        print("config view:", title)
        def chip(src):
            return f"[...document.querySelectorAll('.pane-config .chip')].find(b=>b.innerText.trim().toUpperCase()==='{src.upper()}')"
        wiki0 = await ev(f"{chip('Wiki')}?.classList.contains('is-on')")
        uex0 = await ev(f"{chip('UEX')}?.classList.contains('is-on')")
        print("initial: wiki is-on:", wiki0, "uex is-on:", uex0)
        await ev(f"{chip('UEX')}.click()")
        await asyncio.sleep(1.0)
        wiki1 = await ev(f"{chip('Wiki')}?.classList.contains('is-on')")
        uex1 = await ev(f"{chip('UEX')}?.classList.contains('is-on')")
        stub1 = await ev("document && (function(){const s=document.querySelector('.pane-config');return s? 'view-open':'view-closed'})()")
        print("after UEX click: wiki is-on:", wiki1, "uex is-on:", uex1)
        shot = await c2("Page.captureScreenshot", {"format": "png"})
        p1 = str(OUT / "source-switch-uex.png")
        open(p1, "wb").write(base64.b64decode(shot["result"]["data"]))
        # zurück auf Wiki
        await ev(f"{chip('Wiki')}.click()")
        await asyncio.sleep(1.0)
        wiki2 = await ev(f"{chip('Wiki')}?.classList.contains('is-on')")
        uex2 = await ev(f"{chip('UEX')}?.classList.contains('is-on')")
        print("after Wiki click: wiki is-on:", wiki2, "uex is-on:", uex2)
        shot = await c2("Page.captureScreenshot", {"format": "png"})
        p2 = str(OUT / "source-switch-wiki.png")
        open(p2, "wb").write(base64.b64decode(shot["result"]["data"]))
        ok = title == "KONFIGURATION" and wiki0 is True and uex0 is False \
            and wiki1 is False and uex1 is True and wiki2 is True and uex2 is False
        print("RESULT:", "PASS" if ok else "FAIL", "| shots:", p1, p2)
        rt.cancel()
    srv.terminate(); chr.terminate()

asyncio.run(main())
