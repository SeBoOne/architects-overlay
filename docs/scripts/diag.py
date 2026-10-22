#!/usr/bin/env python3
"""Quick diagnostic: boot, wait for idle pane, type 'gold', dump invokes + errors."""
import asyncio, base64, json, os, pathlib, sys
import urllib.request, websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", "/home/sebo/Projects/Architects Overlay"))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
URL = "http://127.0.0.1:8795/"

async def main():
    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9224", "--user-data-dir=/tmp/cdp-diag",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9224/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    print("CDP ws:", ws)
    logs = []
    async with websockets.connect(ws, max_size=None) as cdp:
        mid = {"n": 0}
        pending = {}
        async def call(m, p=None):
            mid["n"] += 1; cur = mid["n"]
            fut = asyncio.get_event_loop().create_future(); pending[cur] = fut
            await cdp.send(json.dumps({"id": cur, "method": m, "params": p or {}}))
            return await fut
        async def recv_loop():
            async for raw in cdp:
                msg = json.loads(raw)
                if "id" in msg and msg["id"] in pending:
                    pending[msg["id"]].set_result(msg)
                elif msg.get("method") in ("Runtime.consoleAPICalled", "Runtime.exceptionThrown", "Log.entryAdded", "Runtime.uncaughtExceptionThrown"):
                    logs.append(msg.get("method") + ": " + json.dumps(msg.get("params", {}))[:400])
        asyncio.create_task(recv_loop())
        def ok(r):
            if "error" in r: raise RuntimeError(str(r["error"]))
            return r.get("result", {})
        async def js(e):
            r = ok(await call("Runtime.evaluate", {"expression": e, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                ed = r["exceptionDetails"]
                logs.append("EXC: " + ed.get("text","") + " " + (ed.get("exception",{}).get("description","") or "")[:300])
                return None
            return r.get("result", {}).get("value")
        await call("Page.enable")
        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride", {"width":1920,"height":1080,"deviceScaleFactor":2,"mobile":False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.0)
        # instrument invoke logging
        await js("""
          (function(){
            if(window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke && !window.__TAURI_INTERNALS__.invoke.__logged){
              var orig = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
              window.__TAURI_INTERNALS__.invoke = function(cmd,args){
                window.__calls = window.__calls || [];
                window.__calls.push(cmd + ': ' + JSON.stringify((args||{}).query||(args||{}).expr||args||{}));
                return orig(cmd,args);
              };
              window.__TAURI_INTERNALS__.invoke.__logged = true;
            }
          })();
        """)
        # wait for idle pane (boot done)
        end = asyncio.get_event_loop().time() + 12
        phase = None
        while asyncio.get_event_loop().time() < end:
            phase = await js("document.querySelector('.pane-idle') ? 'idle' : (document.querySelector('.pane-loading') ? 'loading' : (document.querySelector('.pane-error') ? 'error' : 'none'))")
            if phase == 'idle': break
            await asyncio.sleep(0.4)
        print("BOOT phase pane:", phase)
        print("smode:", await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : ''"))
        print("data-window:", await js("document.documentElement.dataset.window"))
        print("found bar:", await js("document.querySelector('.found') ? document.querySelector('.found').innerText.replace(/\\s+/g,' ').trim() : 'none'"))
        print("list:", await js("document.querySelector('.list') ? document.querySelector('.list').innerText.slice(0,120) : 'none'"))
        # type gold
        await js("document.querySelector('input.q').focus()")
        expr = ("(function(){var i=document.querySelector('input.q');"
                "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                "set.call(i,'');var ev=new InputEvent('input',{bubbles:true,inputType:'insertText',data:''});"
                "i.dispatchEvent(ev);set.call(i,'gold');"
                "var ev2=new InputEvent('input',{bubbles:true,inputType:'insertText',data:'gold'});i.dispatchEvent(ev2);"
                "return i.value;})()")
        await js(expr)
        await asyncio.sleep(1.2)
        print("AFTER gold input:")
        print("  calls:", await js("JSON.stringify(window.__calls||[])"))
        print("  rows:", await js("document.querySelectorAll('.row').length"))
        print("  vgroup:", await js("document.querySelectorAll('.vgroup').length"))
        print("  found:", await js("document.querySelector('.found .c') ? document.querySelector('.found .c').innerText.replace(/\\s+/g,' ').trim() : 'none'"))
        print("  pane-error:", await js("document.querySelector('.pane-error') ? document.querySelector('.pane-error').innerText.slice(0,200) : 'none'"))
        print("  list text:", await js("document.querySelector('.list') ? document.querySelector('.list').innerText.slice(0,200) : 'none'"))
        print("  smode:", await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : ''"))
    proc.terminate()
    print("\n--- LOGS (", len(logs), ") ---")
    for l in logs[:40]:
        print("  ", l[:300])
asyncio.run(main())
