#!/usr/bin/env python3
"""CDP-Probe für RB3 (Karte t_4e581d1d): „Nächst beste Alternativrouten" im
Routen-Detail. Echte Svelte-Oberfläche, Tauri-IPC gestubbt (smoke-stub.js).

Verifiziert auf der echten UI:
  1. Detail einer Route (Altruciatoxin) zeigt 3 Alternativen gleicher Ware —
     sortiert nach Gewinn/SCU (absteigend), inkl. Buy→Sell-Terminal,
     Gewinn/SCU und SCU-Chips (Enrichment-≋ + Budget-Menge).
  2. Ein-Klick-Wechsel: Klick auf eine Alternative übernimmt die bestehende
     Routen-Ansicht (KAUF-Terminal + Zahlen wechseln).
  3. Fall <2 Alternativen: dezent „keine weiteren" statt leerem Panel
     (Distilled = genau 1 Alternative, Agricultural = 0).

Die Probe aktiviert den __RB3_EXTRA__-Zusatzdatensatz des Stubs (mehr Routen
gleicher Ware) — RB1/RB2-Fixtures bleiben unverändert."""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/routes-ux-rb3")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

# Detail-Expresssion (validiert separat im Browser): sichtbare Zustandsdaten
# des Routen-Details inkl. der Alternativen-Zeilen.
DETAIL_EXPR = ("JSON.stringify((function(){"
    "var h2=document.querySelector('.rdetail h2');"
    "var stops=document.querySelectorAll('.rd-leg .rd-stop');"
    "var rows=[...document.querySelectorAll('.rd-alt-row')].map(function(b){return {"
    "term:(b.querySelector('.rd-alt-term')||{}).textContent||'',"
    "gain:(b.querySelector('.rd-alt-gain')||{}).textContent||'',"
    "chips:[...b.querySelectorAll('.r-chip')].map(function(c){return c.textContent.trim()})"
    "};});"
    "var none=document.querySelector('.rd-alt-none');"
    "return {title:h2?h2.textContent.trim():'',"
    "buy:stops[0]?(stops[0].querySelector('.rd-term')||{}).textContent||'':null,"
    "sell:stops[1]?(stops[1].querySelector('.rd-term')||{}).textContent||'':null,"
    "rows:rows,none:none?none.textContent.trim():''};})())")

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
        "--remote-debugging-port=9233", "--user-data-dir=/tmp/cdp-routes-ux-rb3",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9233/json/list", timeout=1) as r:
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

        async def open_top_route():
            # Klick statt Enter: nach set_input liegt der Fokus ggf. in einem
            # number-Input (inStepper) — dort schluckt der List-Handler das Enter.
            await js("document.querySelector('[data-route-idx=\"0\"]')?.click();")
            await asyncio.sleep(0.8)

        async def set_input(sel, text):
            return await js("(function(){var i=document.querySelector(" + json.dumps(sel) + ");"
                            "if(!i) return 'NO-ELEMENT';"
                            "i.focus();"
                            "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                            "set.call(i,'' );"
                            "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                            "set.call(i," + json.dumps(text) + ");"
                            "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                            "return i.value;})()")

        async def detail_state():
            return json.loads(await js(DETAIL_EXPR))

        # __RB3_EXTRA__ VOR dem Öffnen des Routen-Tabs — der Stub reicht dann
        # zusätzliche Routen gleicher Ware (insgesamt 7 = 4 Altruciatoxin,
        # 2 Distilled, 1 Agricultural).
        await js("globalThis.__RB3_EXTRA__ = true;")
        # Hangar: genau 1 Schiff (Nomad) → auto-selected, SCU 24.
        await js("window.__HANGAR_DATA__ = [{id:1,name:'Nomad',custom_name:'Rennsemmel',origin:'',insurance:null,"
                 "entity_type:'ship',base_ship:'',ship_code:'',manufacturer:'',purchase_price:null,"
                 "purchase_date:'',pledge_id:'',pledge_name:'',warbond:false,ccud:false}];")

        # ---- F2 → Market → ROUTEN ----
        await key("F2")
        await asyncio.sleep(1.0)
        await key("Tab")
        await asyncio.sleep(1.5)
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("rb3-matrix-routes", rows == 7, "rows=" + str(rows))

        # Budget 50k: kaufbare Menge je Route gekappt → Menge-Chips auch in den
        # Alternativen-Zeilen (min SCU, floor 50000/buy). Keine Menge 0 hier.
        await set_input('input[aria-label="Budget (aUEC) — kappt kaufbare Menge"]', "50000")
        await asyncio.sleep(1.5)

        # Row 0 = Tram & Myers (Altruciatoxin, Gain/SCU 3250 — höchster).
        first0 = await js("document.querySelector('[data-route-idx=\"0\"] .m-title')?.textContent || ''")
        check("rb3-top-row-altruciatoxin", first0.startswith("Altruciatoxin"), first0)

        # ---- Detail öffnen (Klick auf oberste Route) ----
        await open_top_route()
        d1 = await detail_state()
        check("rb3-detail-commodity", d1["title"].startswith("Altruciatoxin"), d1["title"])
        check("rb3-detail-buy-tram", d1["buy"] == "Tram & Myers", d1["buy"])
        check("rb3-alternatives-3-rows", len(d1["rows"]) == 3, json.dumps(d1["rows"]))
        # Sortiert nach Gewinn/SCU absteigend: Checkmate 3000, Grim 2950, Orbituary 2850.
        order_ok = (
            "Checkmate Admin" in d1["rows"][0]["term"] and "+3.000" in d1["rows"][0]["gain"]
            and "Grim HEX" in d1["rows"][1]["term"] and "Orbituary" in d1["rows"][2]["term"])
        check("rb3-alternatives-sorted-profit", order_ok, json.dumps(d1["rows"]))
        # SCU-Chips in den Alternativen: Budget-Menge-Chip (RB2) + Enrichment-≋ (C3b).
        chips_ok = all(any(c.startswith("Menge") for c in r["chips"]) for r in d1["rows"])
        vol_ok = any(any(c.startswith("≋") for c in r["chips"]) for r in d1["rows"])
        check("rb3-alt-chips-menge", chips_ok, json.dumps(d1["rows"]))
        check("rb3-alt-chips-enrichment-vol", vol_ok, json.dumps(d1["rows"]))
        # Menge korrekt: 50000/3200=15.6 → floor 15 (Checkmate ist erste Alternative).
        check("rb3-alt-menge-15", "Menge 15 SCU" in d1["rows"][0]["chips"][0], json.dumps(d1["rows"][0]["chips"]))
        check("rb3-alt-none-empty", d1["none"] == "", repr(d1["none"]))
        await shot("01-rb3-detail-alternatives.png")

        # ---- Ein-Klick-Wechsel auf die erste Alternative (Checkmate) ----
        await js("document.querySelectorAll('.rd-alt-row')[0].click();")
        await asyncio.sleep(0.8)
        d2 = await detail_state()
        check("rb3-switch-buy-checkmate", d2["buy"] == "Checkmate Admin", d2["buy"])
        check("rb3-switch-commodity-stays", d2["title"].startswith("Altruciatoxin"), d2["title"])
        # Jetzt ist Tram & Myers die erste Alternative (3250 > 2950 > 2850).
        check("rb3-switch-alt-top-tram", "Tram & Myers" in d2["rows"][0]["term"] and "+3.250" in d2["rows"][0]["gain"], json.dumps(d2["rows"][0]))
        await shot("02-rb3-switched-checkmate.png")

        # ---- <2 Alternativen: Distilled (genau 1) → „keine weiteren" ----
        await key("Escape")
        await asyncio.sleep(0.6)
        await set_input('input[aria-label="Ware filtern"]', "Distilled")
        await asyncio.sleep(1.5)
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("rb3-distilled-2-rows", rows == 2, "rows=" + str(rows))
        await open_top_route()
        d3 = await detail_state()
        check("rb3-distilled-detail", d3["title"].startswith("Distilled Spirits"), d3["title"])
        check("rb3-distilled-no-list", len(d3["rows"]) == 0, json.dumps(d3["rows"]))
        check("rb3-distilled-keine-weiteren", d3["none"] == "keine weiteren für Distilled Spirits", repr(d3["none"]))
        await shot("03-rb3-distilled-keine-weiteren.png")

        # ---- 0 Alternativen: Agricultural Supplies → „keine weiteren" ----
        await key("Escape")
        await asyncio.sleep(0.6)
        await set_input('input[aria-label="Ware filtern"]', "Agricultural")
        await asyncio.sleep(1.5)
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("rb3-agricultural-1-row", rows == 1, "rows=" + str(rows))
        await open_top_route()
        d4 = await detail_state()
        check("rb3-agricultural-keine-weiteren", d4["none"] == "keine weiteren für Agricultural Supplies", repr(d4["none"]))
        check("rb3-agricultural-no-rows", len(d4["rows"]) == 0, json.dumps(d4["rows"]))
        await shot("04-rb3-agricultural-keine-weiteren.png")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS, {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())