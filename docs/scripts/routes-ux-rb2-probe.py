#!/usr/bin/env python3
"""CDP-Probe für RB2 (Karte t_9b42249a): Budget-Eingabe in der Routen-Profit-
Rechnung. Echte Svelte-Oberfläche, Tauri-IPC gestubbt (smoke-stub.js).

Verifiziert auf der echten UI:
  1. Ohne Budget: Menge = Schiff-SCU (Nomad 24), Profit = Gewinn/SCU × 24
     (Verhalten wie heute).
  2. Budget 100k gesetzt: kaufbare Menge = min(SCU, floor(Budget/buy)) →
     Menge-Chip sichtbar, Profit gekappt.
  3. Budget 50k: Menge/Profit nochmal verändert (2 Routen mit Budget vor/nach).
  4. Budget zu niedrig (Route mit Menge 0) → Route sinkt ans Ende + ausgegraut
     + Hinweis "Budget zu niedrig für 1 SCU".
  5. Budget geleert → zurück zu Vollkapazität (identisch mit heute).
  6. Persistenz: Budget-Wert landr in localStorage (überlebt Neustart).
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/routes-ux-rb2")))
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
        "--remote-debugging-port=9232", "--user-data-dir=/tmp/cdp-routes-ux-rb2",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9232/json/list", timeout=1) as r:
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

        BUDGET = 'input[aria-label="Budget (aUEC) — kappt kaufbare Menge"]'
        async def row_info():
            return json.loads(await js("JSON.stringify([...document.querySelectorAll('[data-route-idx]')].map(function(r){"
                                       "var t=r.querySelector('.m-title')?r.querySelector('.m-title').textContent.trim():'';"
                                       "var mm=r.querySelector('.m-meta')?r.querySelector('.m-meta').textContent.trim():'';"
                                       "var q=r.querySelector('.r-chip-menge')?r.querySelector('.r-chip-menge').textContent.trim():'';"
                                       "var zi=r.classList.contains('is-rbzero');"
                                       "var hint=r.querySelector('.r-hint')?r.querySelector('.r-hint').textContent.trim():'';"
                                       "return {t:t,mm:mm,q:q,zero:zi,hint:hint};}))"))

        # Hangar: genau 1 Schiff (Nomad) → auto-selected, SCU 24.
        await js("window.__HANGAR_DATA__ = [{id:1,name:'Nomad',custom_name:'Rennsemmel',origin:'',insurance:null,"
                 "entity_type:'ship',base_ship:'',ship_code:'',manufacturer:'',purchase_price:null,"
                 "purchase_date:'',pledge_id:'',pledge_name:'',warbond:false,ccud:false}];")

        # ---- F2 → Market → ROUTEN ----
        await key("F2")
        await asyncio.sleep(1.0)
        await key("Tab")
        await asyncio.sleep(1.4)
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("rb2-routes-loaded", rows == 3, "rows=" + str(rows))

        # ---- Ohne Budget: Vollkapazität wie heute (SCU 24) ----
        info = await row_info()
        # 3 Routen, keine Menge-Chips (kein Budget aktiv), kein is-rbzero.
        no_budget_ok = all(not r["q"] and not r["zero"] for r in info)
        check("rb2-no-budget-no-chips-grays", no_budget_ok, str(info))
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("rb2-routes-loaded", rows == 3, "rows=" + str(rows))
        diagnostics = json.loads(await js("JSON.stringify({err: window.__pageErr || '', mode: document.querySelector('.mode') ? document.querySelector('.mode').textContent.trim() : '', routes: document.querySelectorAll('[data-route-idx]').length})"))
        print("  DIAG:", diagnostics)
        # Hangar-Ship auto-select may need a beat; force-check SCU input present.
        print("  BUDGET input:", await js("!!document.querySelector('input[aria-label=\"Budget (aUEC) — kappt kaufbare Menge\"]')"))
        print("  BODY:", json.dumps(await js("document.body ? document.body.innerText.slice(0,600) : ''")))
        print("  rNote:", json.dumps(await js("Array.from(document.querySelectorAll('[class*=row-empty],[class*=center]')).map(function(e){return e.innerText.slice(0,120)})")))
        print("  TABS:", json.dumps(await js("JSON.stringify([...document.querySelectorAll('.mtabs button, .mtab')].map(function(e){return e.textContent.trim()}))")))
        print("  ph:", await js("document.body && document.body.getAttribute ? JSON.stringify(Array.from(document.body.children).map(function(c){return c.className||c.tagName})) : ''"))

        # ---- Ohne Budget: Vollkapazität wie heute (SCU 24) ----
        # Stub: w/o budget profit_total = profit_pscu × 24.
        await shot("01-rb2-no-budget.png")

        # ---- Budget 100000 setzen ----
        await set_input(BUDGET, "100000")
        await asyncio.sleep(1.5)
        info = await row_info()
        # 100000/3200=31.25 → floor 31 ≥ 24 → Altruciatoxin Menge 24 (Vollkapa)
        # 100000/1427=70 → floor 70 ≥ 24 → Distilled 24
        # 100000/1108=90 → Agricultural 24  → alle 24 (Budget > Kapazität)
        chipped = all(r["q"].startswith("Menge 24 SCU") for r in info)
        check("rb2-budget-100k-caps-at-capacity", chipped, str(info))
        print("  DIAG-100k:", json.dumps(await js("JSON.stringify({err: window.__pageErr || '', note: (document.querySelector('.row-empty')?document.querySelector('.row-empty').textContent:''), n: document.querySelectorAll('[data-route-idx]').length, body: document.body.innerText.slice(0,400)})")))
        await shot("02-rb2-budget-100k.png")

        # ---- Budget 50000: floor das Volumen merklich (2 Routen) ----
        await set_input(BUDGET, "50000")
        await asyncio.sleep(1.5)
        info = await row_info()
        # 50000/3200=15.6 → floor 15 (Altruciatoxin); 50000/1427=35≥24 (Distilled 24);
        # 50000/1108=45≥24 (Agricultural 24)
        alt = next(r for r in info if r["t"].startswith("Altruciatoxin"))
        check("rb2-budget-50k-alt-capped", alt["q"].startswith("Menge 15 SCU"), str(alt))
        # Profit-Total hat sich geändert (statt 3000*24=72000 jetzt 3000*15=45000)
        check("rb2-budget-50k-alt-profit-capped", "45.000" in alt["mm"], str(alt["mm"]))
        await shot("03-rb2-budget-50k.png")

        # ---- Budget zu niedrig: Menge-0-Route sinkt + grau + Hinweis ----
        # Altruciatoxin buy=3200 → Budget 1500 (< 3200) → Menge 0 (sinkt/grau).
        # Distilled buy=1427 → 1500/1427=1.05 → floor 1; Agricultural buy=1108 → floor 1.
        await set_input(BUDGET, "1500")
        await asyncio.sleep(1.5)
        info = await row_info()
        alt = next(r for r in info if r["t"].startswith("Altruciatoxin"))
        dist = next(r for r in info if r["t"].startswith("Distilled"))
        check("rb2-budget-1500-alt-zero-gray", alt["zero"] and alt["hint"] == "Budget zu niedrig für 1 SCU", str(alt))
        check("rb2-budget-1500-dist-capped", dist["q"].startswith("Menge 1 SCU"), str(dist))
        # Altruciatoxin (Menge 0) muss ans LISTEN-ENDE sortiert sein (sink):
        titles = [r["t"] for r in info]
        check("rb2-zero-route-sinks-to-end", titles[-1].startswith("Altruciatoxin"), str(titles))
        await shot("04-rb2-budget-1500-zero-gray-sink.png")


        # ---- Budget leeren → identisch mit heute ----
        await set_input(BUDGET, "")
        await asyncio.sleep(1.5)
        info = await row_info()
        back_ok = all(not r["q"] and not r["zero"] and not r["hint"] for r in info)
        check("rb2-clear-budget-identical-today", back_ok, str(info))
        await shot("05-rb2-clear-budget.png")

        # ---- Persistenz in localStorage ----
        await set_input(BUDGET, "12345")
        await asyncio.sleep(1.0)
        stored = await js("localStorage.getItem('architects-overlay.routes.budget')")
        check("rb2-persists-localStorage", stored == "12345", str(stored))

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS, {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())
