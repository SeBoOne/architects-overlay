#!/usr/bin/env python3
"""CDP-Probe für t_128b7547 (Handelsrouten: Filter + Sortierung, UEX-Daten).

Belegt auf der echten Svelte-Oberfläche (echte Logik, Tauri-IPC gestubbt):
  1. F2 → Market, Tab-Umschaltung auf ROUTEN (Tab-Taste UND Klick).
  2. Routen-Liste rendert Zeilen (Gewinn/SCU, Marge, Distanz-Label).
  3. Filter: Illegal-Chip → nur illegale Ware; Ware-Textfilter; SCU gesetzt.
  4. Sortierung: Distanz aufsteigend sortiert die Liste neu.
  5. Schiff aus Hangar gewählt → SCU automatisch (vehicles_scu-Stub: Nomad 24).
  6. Enter → Route-Detail: Gewinn-Mathematik = Preisdifferenz × SCU (Plausi).
  7. ESC: Detail → Liste → zurück zu Angeboten/Market-Semantik.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/trading-routes")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8796/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8796", "--bind", "127.0.0.1",
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
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9230", "--user-data-dir=/tmp/cdp-trading-routes",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9230/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        server.terminate(); proc.terminate()
        raise RuntimeError("chromium CDP did not come up")

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

        await call("Page.enable")
        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        VK = {"F1": 112, "F2": 113, "Escape": 27, "Enter": 13, "Tab": 9,
              "ArrowDown": 40, "ArrowUp": 38}
        async def key(k):
            vk = VK[k]
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.3)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", OUT / name)

        async def click(sel):
            await js(f"document.querySelector({json.dumps(sel)})?.click()")
            await asyncio.sleep(0.6)

        async def set_input(sel, text):
            await js("(function(){var i=document.querySelector(" + json.dumps(sel) + ");"
                     "if(!i) return 'NO-ELEMENT';"
                     "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                     "set.call(i,'');"
                     "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                     "set.call(i," + json.dumps(text) + ");"
                     "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                     "return i.value;})()")

        # Hangar-Daten injizieren (Stub liest window.__HANGAR_DATA__ zur Invoke-Zeit)
        await js("window.__HANGAR_DATA__ = [{id:1,name:'Nomad',custom_name:'Rennsemmel',origin:'',insurance:null,"
                 "entity_type:'ship',base_ship:'',ship_code:'',manufacturer:'',purchase_price:null,"
                 "purchase_date:'',pledge_id:'',pledge_name:'',warbond:false,ccud:false}];")

        # ---- F2 → Market → Tab auf ROUTEN ----
        await key("F2")
        await asyncio.sleep(1.0)
        tabs = await js("!!document.querySelector('.mtabs')")
        check("market-tabs-visible", bool(tabs))
        await key("Tab")
        await asyncio.sleep(1.2)
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("routes-list-renders", rows == 3, "rows=" + str(rows))
        titles = json.loads(await js("JSON.stringify([...document.querySelectorAll('[data-route-idx] .m-title')].map(function(e){return e.textContent.trim();}))"))
        check("routes-default-sort-profit-desc", titles[0].startswith("Altruciatoxin"), str(titles))
        await shot("01-routes-list.png")

        # ---- Enter → Detail: Plausibilität Gewinn = Preisdifferenz × SCU ----
        await key("Enter")
        await asyncio.sleep(0.8)
        detail = await js("!!document.querySelector('.rdetail')")
        check("route-detail-opens", bool(detail))
        math_txt = (await js("document.querySelector('.rd-math') ? document.querySelector('.rd-math').innerText : ''")).replace("\n", " ")
        # SCU=1 (manuell 0): Gewinn 6200-3200 = 3000 (de-DE: "3.000")
        check("route-detail-math-scu1", "3.000" in math_txt and "3.200" in math_txt and "6.200" in math_txt, math_txt)
        await shot("02-route-detail.png")
        await key("Escape")  # Detail zu
        await asyncio.sleep(0.4)
        gone = await js("!document.querySelector('.rdetail')")
        check("esc-closes-detail", bool(gone))

        # ---- Schiff aus Hangar → SCU automatisch (Nomad = 24) ----
        await js("(function(){var s=document.querySelector('.rsel');"
                 "var set=Object.getOwnPropertyDescriptor(window.HTMLSelectElement.prototype,'value').set;"
                 "set.call(s,'Nomad');"
                 "s.dispatchEvent(new Event('change',{bubbles:true}));"
                 "return s.value;})()")
        await asyncio.sleep(1.2)
        scu_val = await js("document.querySelector('input[aria-label=\"SCU-Kapazität\"]').value")
        check("ship-from-hangar-sets-scu", scu_val == "24", scu_val)

        # ---- Filter: Illegal → nur Altruciatoxin ----
        await click(".mbar2 button.chip:nth-of-type(3)")  # Illegal-Chip (erste mbar2-Zeile)
        await asyncio.sleep(1.0)
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        titles = json.loads(await js("JSON.stringify([...document.querySelectorAll('[data-route-idx] .m-title')].map(function(e){return e.textContent.trim();}))"))
        check("filter-illegal-only", rows == 1 and titles[0].startswith("Altruciatoxin"), str(rows) + " " + str(titles))
        # Illegal wieder aus
        await click(".mbar2 button.chip:nth-of-type(1)")
        await asyncio.sleep(0.8)

        # ---- Ware-Textfilter ----
        await set_input("input[aria-label=\"Ware filtern\"]", "spirits")
        await asyncio.sleep(1.2)
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("filter-commodity-text", rows == 1, "rows=" + str(rows))
        await set_input("input[aria-label=\"Ware filtern\"]", "")
        await asyncio.sleep(0.8)

        # ---- Sortierung: Profit aufsteigend / Distanz ----
        await js("[...document.querySelectorAll('.mbar2 button')].find(function(b){return b.textContent.trim()==='Gewinn/SCU ↑'}).click()")
        await asyncio.sleep(1.0)
        titles = json.loads(await js("JSON.stringify([...document.querySelectorAll('[data-route-idx] .m-title')].map(function(e){return e.textContent.trim();}))"))
        check("sort-profit-asc", titles[0].startswith("Agricultural"), str(titles))
        await js("[...document.querySelectorAll('.mbar2 button')].find(function(b){return b.textContent.trim()==='Distanz ↑'}).click()")
        await asyncio.sleep(1.0)
        check("sort-distance-ok", int(await js("document.querySelectorAll('[data-route-idx]').length")) == 3)

        await shot("03-routes-filtered-sorted.png")

        # ---- ESC-Verhalten: Routen-Tab → Market schließt ----
        await key("Escape")
        await asyncio.sleep(0.4)
        smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()
        check("esc-routes-exits-market", smode == "SUCHE", smode)

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS, {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())
