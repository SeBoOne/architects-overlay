#!/usr/bin/env python3
"""Drive headless Chromium over CDP to capture overlay smoke screenshots."""
import asyncio, base64, json, os, pathlib
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", "/home/sebo/Projects/Architects Overlay"))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", "/tmp/smoke-shots"))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8795/"

async def main():
    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium",
        "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9223", "--user-data-dir=/tmp/cdp-smoke",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL,
    )
    import urllib.request
    ws = None
    for _ in range(60):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9223/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        raise RuntimeError("chromium CDP page target did not come up")

    async with websockets.connect(ws, max_size=None) as cdp:
        msg_id = 0
        pending = {}

        async def call(method, params=None):
            nonlocal msg_id
            msg_id += 1
            mid = msg_id
            fut = asyncio.get_event_loop().create_future()
            pending[mid] = fut
            await cdp.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
            return await fut

        console_events = []
        async def recv_loop():
            async for raw in cdp:
                msg = json.loads(raw)
                if "id" in msg and msg["id"] in pending:
                    pending[msg["id"]].set_result(msg)
                elif msg.get("method") == "Runtime.exceptionThrown":
                    d = msg["params"]["exceptionDetails"]
                    console_events.append("EXC: " + (d.get("text") or "") + " " + (d.get("exception",{}).get("description","") or "")[:300])
                elif msg.get("method") == "Log.entryAdded":
                    e = msg["params"]["entry"]
                    console_events.append("LOG: " + e.get("text","")[:200])
        asyncio.create_task(recv_loop())

        def ok(resp):
            if "error" in resp:
                raise RuntimeError("CDP error: %s" % resp["error"])
            return resp.get("result", {})

        await call("Page.enable")
        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            v = r.get("result", {}).get("value")
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return v

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = base64.b64decode(r["data"])
            (OUT / (name + ".png")).write_bytes(data)
            print("  shot:", OUT / (name + ".png"), len(data), "bytes")

        async def typeQuery(text):
            await js("document.querySelector('input.q').focus()")
            # set value through the prototype setter then fire a real InputEvent
            # that carries inputType/data so Svelte 5's bind:value updates the rune
            expr = (
                "(function(){"
                "var i=document.querySelector('input.q');"
                "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                "set.call(i,'');"
                "var ev=new InputEvent('input',{bubbles:true,inputType:'insertText',data:''});"
                "i.dispatchEvent(ev);"
                "set.call(i," + json.dumps(text) + ");"
                "var ev2=new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "});"
                "i.dispatchEvent(ev2);"
                "return i.value;"
                "})()"
            )
            await js(expr)
            await asyncio.sleep(0.7)

        # instrument invoke logging
        await js("""
          (function(){
            var orig = window.__TAURI_INTERNALS__.invoke;
            window.__calls = [];
            window.__TAURI_INTERNALS__.invoke = function(cmd,args){ window.__calls.push(cmd+':'+JSON.stringify((args||{}).query||(args||{}).expr||'')); return orig(cmd,args); };
          })();
        """)
        # State 1: idle
        print("state: idle")
        print("  helper:", await js("document.querySelector('.helper') ? document.querySelector('.helper').innerText.replace(/\\s+/g,' ').trim() : 'NONE'"))
        print("  window-attr:", await js("document.documentElement.dataset.window"))
        await shot("01-idle")

        # warm-up: first real keystroke after hydration (mirrors a user's very
        # first interaction) so the subsequent deterministic searches render
        await typeQuery("WARM")  # runs a debounced search, then cleared
        await typeQuery("")      # ensure cleared
        # State 2: search 'gold'
        # Under CDP automation the very first synthetic `input` after boot
        # sometimes doesn't flush Svelte's render (verified: identical second
        # query renders). Re-type once more deterministically.
        await typeQuery("gold")
        await typeQuery("gold")
        await asyncio.sleep(0.5)
        print("search 'gold' input val=", await js("document.querySelector('input.q').value"),
              "smode=", await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : ''"))
        print("  groups=", await js("document.querySelectorAll('.vgroup').length"),
              "rows=", await js("document.querySelectorAll('.row').length"),
              "badges=", await js("[...document.querySelectorAll('.rbadge')].map(function(b){return b.textContent}).join(',')"),
              "found=", await js("document.querySelector('.found .c') ? document.querySelector('.found .c').innerText.replace(/\\s+/g,' ').trim() : ''"))
        await shot("02-search-gold")

        # State 3: expand first row
        await js("var el=document.querySelector('.row'); el && el.click()")
        await asyncio.sleep(0.8)
        print("expanded:", await js("document.querySelectorAll('.rwrap.is-open').length"),
              "buy+price rows:", await js("document.querySelectorAll('.ptab .pr').length"))
        await shot("03-expanded-gold")

        # State 4: math
        await typeQuery("=2*250+15%")
        print("calc:", await js("document.querySelector('.found .c') ? document.querySelector('.found .c').innerText : ''"))
        await shot("04-calc")

        # State 5: prefix :i
        await typeQuery(":i gold")
        await typeQuery(":i gold")
        print("prefix :i vgroups:", await js("[...document.querySelectorAll('.vgroup')].map(function(g){return g.innerText.replace(/\\s+/g,' ').trim()}).join('|')"))
        await shot("05-prefix-item")

        # State 6: Alt+Zahl focus
        await typeQuery("gold")
        await js("document.dispatchEvent(new KeyboardEvent('keydown',{key:'3',altKey:true,bubbles:true}))")
        await asyncio.sleep(0.5)
        print("is-focus idx:", await js("[...document.querySelectorAll('.row')].findIndex(function(r){return r.classList.contains('is-focus')})"))
        print("  [6] groups=", await js("document.querySelectorAll('.vgroup').length"),
              "rows=", await js("document.querySelectorAll('.row').length"),
              "badges=", await js("[...document.querySelectorAll('.rbadge')].map(function(b){return b.textContent}).join(',')"),
              "found=", await js("document.querySelector('.found .c') ? document.querySelector('.found .c').innerText.replace(/\\s+/g,' ') : ''"))
        await shot("06-alt3-focus")
        print("IPC CALLS:", await js("JSON.stringify(window.__calls||[])"))
        print("CONSOLE EVENTS:")
        for ce in console_events[:30]:
            print("  ", ce)

        # State 7: Checkmate search -> Location-Entity (kind='location', id=436)
        # und Expansion des Standort-aggregats. Screenshot der Location-Entity.
        await typeQuery("checkmate")
        await asyncio.sleep(0.8)
        print("search 'checkmate' groups=", await js("document.querySelectorAll('.vgroup').length"),
              "rows=", await js("document.querySelectorAll('.row').length"),
              "badges=", await js("[...document.querySelectorAll('.rbadge')].map(function(b){return b.textContent}).join(',')"))
        # Location row = kind='location' (erste .row der Locations-Gruppe).
        await js("var loc=document.querySelector('.row[data-kind=\"location\"]'); "
                 "if(!loc){var rows=document.querySelectorAll('.row');loc=[...rows].find(function(r){return r.textContent.includes('Checkmate Station')&&![...r.classList].includes('kind-terminal')});} "
                 "if(loc)loc.click();")
        await asyncio.sleep(1.0)
        print("  [7] open location groups=", await js("document.querySelectorAll('.rwrap.is-open').length"),
              "price rows=", await js("document.querySelectorAll('.ptab .pr').length"))
        print("  [7] IPC calls for checkmate=", await js("JSON.stringify(window.__calls||[])"))
        await shot("07-checkmate-location")

    proc.terminate()

asyncio.run(main())
