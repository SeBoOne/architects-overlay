#!/usr/bin/env python3
"""Verify Punkt 6 (Detail-Markierung) gegen den echten Stub: ↑↓ setzt is-sel,
Enter setzt is-chosen — deklarativ statt DOM-Query, nach dem Svelte-Flush."""
import asyncio, base64, json, os, pathlib, subprocess, time, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", "/home/sebo/Projects/Architects Overlay"))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screenshots/v0.1.5"
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

    subprocess.run(["pkill", "-f", "remote-debugging-port=9224"], stderr=subprocess.DEVNULL)
    chr = subprocess.Popen(
        ["/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
         "--window-size=1920,1080", "--remote-debugging-port=9224", "--user-data-dir=/tmp/cdp-sel5",
         "about:blank"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ws = None
    for _ in range(60):
        try:
            pages = [t for t in json.load(urllib.request.urlopen("http://127.0.0.1:9224/json/list", timeout=1)) if t.get("type") == "page"]
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
        # search gold, open detail row 0
        await ev("(()=>{const i=document.querySelector('input');i.focus();const s=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;s.call(i,'gold');i.dispatchEvent(new Event('input',{bubbles:true}));})()")
        await asyncio.sleep(1.5)
        await ev("document.querySelector('.row')?.click()")
        await asyncio.sleep(1.2)
        n = await ev("document.querySelectorAll('.pane-detail .pr').length")
        print("detail .pr count:", n)
        # initial: no is-sel
        s0 = await ev("document.querySelectorAll('.pane-detail .pr.is-sel').length")
        print("initial is-sel:", s0)
        # ArrowDown -> dMove -> is-sel should be 1
        await ev("window.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown'}))")
        await asyncio.sleep(0.9)  # let a flush happen
        s1 = await ev("document.querySelectorAll('.pane-detail .pr.is-sel').length")
        print("after ArrowDown is-sel:", s1)
        selidx = await ev("[...document.querySelectorAll('.pane-detail .pr')].findIndex(r=>r.classList.contains('is-sel'))")
        print("is-sel on row index:", selidx)
        # Enter -> dSelect -> is-chosen should be 1
        await ev("window.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter'}))")
        await asyncio.sleep(0.9)
        c1 = await ev("document.querySelectorAll('.pane-detail .pr.is-chosen').length")
        print("after Enter is-chosen:", c1)
        toast = await ev("document.querySelector('.d-toast')?.innerText||''")
        print("toast:", toast)
        # wait another flush, confirm classes survived (the QA bug: DOM-query classes died on flush)
        await asyncio.sleep(1.0)
        s2 = await ev("document.querySelectorAll('.pane-detail .pr.is-sel').length")
        c2n = await ev("document.querySelectorAll('.pane-detail .pr.is-chosen').length")
        print("after flush -> is-sel:", s2, "is-chosen:", c2n)
        ok = s1 == 1 and c1 == 1 and s2 == 1 and c2n == 1
        shot = await c2("Page.captureScreenshot", {"format": "png"})
        path = str(OUT / "02b-detail-marking.png")
        open(path, "wb").write(base64.b64decode(shot["result"]["data"]))
        print("screenshot:", path)
        print("RESULT:", "PASS" if ok else "FAIL", "is-sel", s2, "is-chosen", c2n)
        rt.cancel()
    srv.terminate(); chr.terminate()

asyncio.run(main())