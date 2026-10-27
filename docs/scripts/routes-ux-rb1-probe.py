#!/usr/bin/env python3
"""CDP-Probe für RB1 (Karte t_723939ff): Routen-Filter = EIN Typahead-Feld
pro Auswahl (Schiff / Startort / Zielort), statt separatem Filterfeld.

Belegt auf der echten Svelte-Oberfläche (echte Logik, Tauri-IPC gestubbt —
docs/scripts/smoke-stub.js mit location_suggest + __locPool + get_routes):
  1. F2 → Market, Tab → ROUTEN.
  2. Schiff-Typeahead: tippen filtert Dropdown → ArrowDown → Enter übernimmt
     → SCU automatisch (applyShipScu über hangarShips/vehicles_scu).
  3. Startort-Typeahead: tippen zeigt NUR Locations (kein Terminal/Shop) →
     Enter übernimmt die Auswahl.
  4. Zielort-Typeahead: dasselbe.
Zusätzlich: Esc schließt das Dropdown ohne Übernahme.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/routes-ux-rb1")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

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
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9231", "--user-data-dir=/tmp/cdp-routes-ux-rb1",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9231/json/list", timeout=1) as r:
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
            await asyncio.sleep(0.35)

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
                     "i.focus();"
                     "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                     "set.call(i,'' );"
                     "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                     "set.call(i," + json.dumps(text) + ");"
                     "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                     "return i.value;})()")

        async def dd_items():
            # Dropdown-Einträge: [role=option]-Buttons im nächsten .rta-dd
            return json.loads(await js("JSON.stringify([...document.querySelectorAll('.rta-dd [role=option]')].map(function(e){return e.textContent.trim();}))"))

        # Hangar-Daten injizieren (Stub liest window.__HANGAR_DATA__ zur Invoke-Zeit)
        await js("window.__HANGAR_DATA__ = [{id:1,name:'Nomad',custom_name:'Rennsemmel',origin:'',insurance:null,"
                 "entity_type:'ship',base_ship:'',ship_code:'',manufacturer:'',purchase_price:null,"
                 "purchase_date:'',pledge_id:'',pledge_name:'',warbond:false,ccud:false}];")

        # ---- F2 → Market → ROUTEN ----
        await key("F2")
        await asyncio.sleep(1.0)
        await key("Tab")  # Angebote → Routen (Karte t_128b7547 Umschaltung)
        await asyncio.sleep(1.2)
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("routes-tab-loaded-rows", rows == 3, "rows=" + str(rows))

        SHIP = 'input[aria-label="Schiff Type-ahead — Tippen filtert, ↵ übernimmt"]'
        START = 'input[aria-label="Startort Type-ahead — Tippen filtert, ↵ übernimmt"]'
        ZIEL = 'input[aria-label="Zielort Type-ahead — Tippen filtert, ↵ übernimmt"]'

        # ---- B1a: Schiff-Typeahead ----
        # kein separates <select> mehr
        has_sel = bool(await js("!!document.querySelector('.mbar2 select.rsel')"))
        check("no-separate-ship-select-removed", not has_sel)
        await set_input(SHIP, "nom")
        await asyncio.sleep(0.8)
        items = await dd_items()
        check("ship-typeahead-filters-dropdown", items == ["Rennsemmel"], str(items))
        await shot("01-ship-typeahead-filtered.png")
        await key("ArrowDown")  # Highlight
        await key("Enter")      # übernehmen
        await asyncio.sleep(0.8)
        ship_val = await js(f"document.querySelector({json.dumps(SHIP)}).value")
        scu_val = await js("document.querySelector('input[aria-label=\"SCU-Kapazität\"]').value")
        check("ship-enter-adopts-and-sets-scu", ship_val == "Rennsemmel" and scu_val == "24",
              "ship=" + str(ship_val) + " scu=" + str(scu_val))
        await shot("02-ship-typeahead-adopted.png")

        # ---- B1b: Startort-Typeahead (Locations only) ----
        await set_input(START, "lev")
        await asyncio.sleep(0.9)
        items = await dd_items()
        check("start-typeahead-location-only", items == ["Levski"], str(items))
        await shot("03-start-typeahead-filtered.png")
        await key("ArrowDown")
        await key("Enter")
        await asyncio.sleep(0.8)
        from_val = await js(f"document.querySelector({json.dumps(START)}).value")
        check("start-enter-adopts-location", from_val == "Levski", str(from_val))

        # ---- B1c: Zielort-Typeahead ----
        await set_input(ZIEL, "check")
        await asyncio.sleep(0.9)
        items = await dd_items()
        check("ziel-typeahead-location-only", items == ["Checkmate Station"], str(items))
        await key("ArrowDown")
        await key("Enter")
        await asyncio.sleep(0.8)
        to_val = await js(f"document.querySelector({json.dumps(ZIEL)}).value")
        check("ziel-enter-adopts-location", to_val == "Checkmate Station", str(to_val))
        await shot("04-start-ziel-adopted-no-separate.png")

        # ---- Esc schließt Dropdown ohne Übernahme ----
        await set_input(START, "gri")
        await asyncio.sleep(0.9)
        items = await dd_items()
        check("start-typeahead-grim", items == ["Grim HEX"], str(items))
        await key("Escape")
        await asyncio.sleep(0.4)
        items = await dd_items()
        check("esc-closes-dropdown-without-adopt", items == [], str(items))

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS, {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())