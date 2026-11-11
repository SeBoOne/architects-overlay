#!/usr/bin/env python3
"""CDP-Probe für t_7a548e85 (M6-F4: Trade-Route-Pin — Commodity-Regel).

Belegt auf der echten Svelte-Oberfläche (echte Logik, Tauri-IPC gestubbt):
  1. Zwei Routen-Pins unterschiedlicher Commodities (Altruciatoxin idx0 +
     Distilled Spirits idx1) via 'p'-Kontextaktion.
  2. Im Altruciatoxin-Detail auf alternativen KAUFORT klicken
     (Tram & Myers): Filter übernimmt den Ort, die Detail-Berechnung
     folgt der neuen Auswahl (GLEICHES Commodity, neuer Preis).
  3. Nach dem Filter-Reload: der Altruciatoxin-Pin zeigt weiterhin
     Altruciatoxin mit Kaufort Tram & Myers (3.100) + neu berechneter
     Marge — NIE ein Sprung auf ein anderes Commodity.
  4. Der Spirits-Pin bleibt UNBERÜHRT (kein pin_update auf box_index 1
     mit fremdem Commodity; kein Index-Fallback nach Reload).
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/m6-f4-pin-key")))
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
        "--remote-debugging-port=9231", "--user-data-dir=/tmp/cdp-m6f4-pin",
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

        VK = {"F2": 113, "F3": 114, "Escape": 27, "Enter": 13, "Tab": 9,
              "ArrowDown": 40, "ArrowUp": 38, "p": 80}
        async def key(k):
            vk = VK[k]
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": ("Key" + k.upper() if k.isalpha() else k),
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": ("Key" + k.upper() if k.isalpha() else k),
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.35)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", OUT / name)

        async def pin_invokes():
            return json.loads(await js(
                "JSON.stringify((globalThis.__invokes||[]).filter(function(x){return x.cmd==='pin_add'||x.cmd==='pin_update';}))"))

        # Hangar: genau 1 Schiff (Nomad) → auto-selected, SCU 24 (wie
        # routes-ux-rb2-probe: ohne Schiff keine Routen-Kapazität → 0 rows).
        await js("window.__HANGAR_DATA__ = [{id:1,name:'Nomad',custom_name:'Rennsemmel',origin:'',insurance:null,"
                 "entity_type:'ship',base_ship:'',ship_code:'',manufacturer:'',purchase_price:null,"
                 "purchase_date:'',pledge_id:'',pledge_name:'',warbond:false,ccud:false}];")

        # ---- F3 → ROUTES-Modus (Routen sind eigener Modus, nicht Market-Tab) ----
        await key("F3")
        await asyncio.sleep(1.4)
        # Filter-Input ist nach dem Moduswechsel fokussiert → 'p' würde als
        # Feld-Eingabe gelten (isFieldFocused). Fokus lösen wie ein Klick
        # in die Liste.
        await js("document.activeElement && document.activeElement.blur()")
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("routes-list-renders", rows == 3, "rows=" + str(rows))
        titles = json.loads(await js("JSON.stringify([...document.querySelectorAll('[data-route-idx] .m-title')].map(function(e){return e.textContent.trim();}))"))
        check("default-order-altru-first", titles[0].startswith("Altruciatoxin") and titles[1].startswith("Distilled"), str(titles))

        # ---- Zwei Pins unterschiedlicher Commodities ----
        await key("p")           # rSel=0 → Altruciatoxin
        await asyncio.sleep(0.8)
        await key("ArrowDown")   # rSel=1 → Distilled Spirits
        await asyncio.sleep(0.3)
        await key("p")
        await asyncio.sleep(0.8)
        inv = await pin_invokes()
        adds = [x for x in inv if x["cmd"] == "pin_add"]
        check("two-pins-added", len(adds) == 2, str(len(adds)))
        check("pin-add-carries-key", all(x["args"].get("key") for x in adds), str([x["args"].get("key") for x in adds]))
        check("pin-keys-distinct-commodity", adds[0]["args"]["key"].split(":")[0] != adds[1]["args"]["key"].split(":")[0], str(adds[0]["args"]["key"]) + " vs " + str(adds[1]["args"]["key"]))
        await shot("01-two-pins-pinned.png")

        # ---- Altruciatoxin-Detail öffnen (rSel zurück auf 0) ----
        await key("ArrowUp")
        await asyncio.sleep(0.3)
        await key("Enter")
        await asyncio.sleep(1.0)
        detail = await js("!!document.querySelector('.rdetail')")
        check("altru-detail-opens", bool(detail))
        head = (await js("document.querySelector('.rdetail .md-t h2') ? document.querySelector('.rdetail .md-t h2').innerText : ''")).strip()
        check("detail-is-altru", head.startswith("Altruciatoxin"), head)

        # ---- Alternativen KAUFORT wählen: Tram & Myers (3100) ----
        await js("document.querySelector('.rd-pair-row[data-side=\"buy\"][data-term-id=\"430\"]')?.click()")
        await asyncio.sleep(2.2)  # Leg-Override + Filter-Reload (debounce 250ms) abwarten

        # (a) Detail-Berechnung folgt der Auswahl — gleiches Commodity
        head2 = (await js("document.querySelector('.rdetail .md-t h2') ? document.querySelector('.rdetail .md-t h2').innerText : ''")).strip()
        check("detail-still-altru", head2.startswith("Altruciatoxin"), head2)
        buy_price = (await js("document.querySelector('.rdetail .rd-leg .rd-stop .rd-price') ? document.querySelector('.rdetail .rd-leg .rd-stop .rd-price').innerText : ''")).strip()
        check("detail-buy-price-3100", "3.100" in buy_price, buy_price)
        math_txt = (await js("document.querySelector('.rd-math') ? document.querySelector('.rd-math').innerText : ''")).replace("\n", " ")
        # Detail-Math folgt der Auswahl: SCU-Kapazität 24 (Nomad) →
        # Purchase 3.100 × 24 = 74.400, Gross 6.200 × 24 = 148.800,
        # Margin 100 %. GLEICHES Commodity, neuer Kaufpreis eingerechnet.
        check("detail-math-follows-selection", "74.400" in math_txt and "148.800" in math_txt and "100.0" in math_txt, math_txt)

        # (b) Pin-Nachzug: Leg-Change-Update NUR für den Altru-Pin
        inv = await pin_invokes()
        ups = [x for x in inv if x["cmd"] == "pin_update"]
        # Erstes pin_update nach dem Klick = Leg-Wechsel (box_index 0)
        leg_ups = [x for x in ups if x["args"]["box_index"] == 0]
        check("leg-change-updates-altru-pin", len(leg_ups) >= 1, str(len(leg_ups)))
        if leg_ups:
            a0 = leg_ups[0]["args"]
            rows0 = a0["rows"]
            check("pin-buy-leg-tram-myers", rows0[0]["name"] == "Tram & Myers", str(rows0[0]))
            check("pin-buy-price-3100", "3.100" in rows0[0]["value"], rows0[0]["value"])
            check("pin-sell-leg-unchanged", "Bai Hang" in rows0[1]["name"], str(rows0[1]))
            check("pin-kpi-recomputed", a0["kpi"] == "+100 %", a0["kpi"])
            check("pin-key-evolves", a0.get("key") == "4:430:428", str(a0.get("key")))
        # Fremdes Commodity: NIE ein pin_update auf den Spirits-Pin
        foreign = [x for x in ups if x["args"]["box_index"] == 1 and "Altruciatoxin" in (x["args"].get("title") or "")]
        check("no-commodity-jump-on-spirits-pin", not foreign, str(foreign))
        # Auch in den ROWS darf kein Fremd-Commodity-Terminal landen —
        # Spirits-Pin bleibt exakt auf seinem Anhefte-Stand:
        st = json.loads(await js("JSON.stringify((globalThis.__pins||[]))"))
        spirits = [p for p in st if p["view_id"] == "routes" and p["box_index"] == 1][0]
        check("spirits-pin-untouched", "Distilled" in spirits["title"] and "Bai Hang" in spirits["rows"][1]["name"] and spirits["rows"][0]["value"] == "1.427" and spirits["rows"][1]["value"] == "2.320", json.dumps(spirits, ensure_ascii=False))
        altru = [p for p in st if p["view_id"] == "routes" and p["box_index"] == 0][0]
        check("altru-pin-still-altru", "Altruciatoxin" in altru["title"], altru["title"])
        await shot("02-after-leg-change.png")

        # (c) Nach Reload: key-first Sync — kein Commodity-Sprung
        # (der Sync-Pin-Refresh ist Teil derselben invoke-Historie oben;
        #  abschließend nochmals den Stand prüfen:)
        altru2 = [p for p in st if p["view_id"] == "routes" and p["box_index"] == 0][0]
        check("altru-pin-key-after-sync", altru2.get("key") == "4:430:428", str(altru2.get("key")))

        # Detail-Kalkulation im Kopf folgt der Auswahl (SCU 24):
        # Leg-Change sofort: (6200-3100) × 24 = 74.400; nach Reload-Resync
        # auf den Backend-Eintrag: (6350-3100) × 24 = 78.000 — beides
        # Altruciatoxin.
        head_profit = (await js("document.querySelector('.rdetail .md-price .v') ? document.querySelector('.rdetail .md-price .v').innerText : ''")).strip()
        check("detail-head-profit-follows", ("74.400" in head_profit) or ("78.000" in head_profit), head_profit)
        # Finaler Pin-Stand nach key-first Sync: gleicher Key, Kauf 3.100.
        st2 = json.loads(await js("JSON.stringify((globalThis.__pins||[]))"))
        altru2 = [p for p in st2 if p["view_id"] == "routes" and p["box_index"] == 0][0]
        check("altru-pin-buy-after-sync", "3.100" in altru2["rows"][0]["value"], altru2["rows"][0])
        check("altru-pin-key-after-sync", altru2.get("key") == "4:430:428", str(altru2.get("key")))

        await shot("03-final-state.png")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS, {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())
