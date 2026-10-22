#!/usr/bin/env python3
"""N2 Enter-Detail-Filter — CDP-Probe (Task t_d9f00c2a).

Beweist am realen Svelte-Build (Stub-IPC):
  (a) Enter auf Location 'Checkmate Station' → Detail offen; Filter 'lama'
      im Suchfeld zeigt nur Lama-Terminal-Content.
  (b) Enter auf Laranite → Preisliste; Filter 'L1' zeigt nur L1-Orte.
  (c) Esc → zurück zur Hauptsuche, Suchfeld leer.
"""
import asyncio, base64, json, os, pathlib
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", "/home/sebo/Projects/Architects Overlay"))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screenshots/n2-enter-filter"
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8795/"

async def main():
    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium",
        "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9224", "--user-data-dir=/tmp/cdp-n2probe",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL,
    )
    import urllib.request
    ws = None
    for _ in range(60):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9224/json/list", timeout=1) as r:
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
                    console_events.append("EXC: " + (d.get("text") or "") + " " + (d.get("exception", {}).get("description", "") or "")[:300])
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
            await asyncio.sleep(0.6)

        async def pressEnter():
            await js("document.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}))")
            await asyncio.sleep(0.8)

        async def pressEsc():
            await js("document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))")
            await asyncio.sleep(0.5)

        results = []
        def check(label, cond, detail=""):
            results.append((label, bool(cond), detail))
            print(("PASS" if cond else "FAIL"), label, detail)

        detail_open = "document.querySelector('.pane-detail') !== null"
        detail_title = "document.querySelector('.d-title h2') ? document.querySelector('.d-title h2').innerText : ''"
        caps = "[...document.querySelectorAll('.pane-detail .ptab .cap')].map(function(e){return e.innerText.replace(/\\s+/g,' ').trim()})"
        prlocs = "[...document.querySelectorAll('.pane-detail .ptab .pr .loc')].map(function(e){return e.innerText.trim()})"
        input_val = "document.querySelector('input.q').value"

        # Warm-up (erste synthetische Eingabe nach Boot flusht manchmal nicht)
        await typeQuery("WARM")
        await typeQuery("")

        # ---- (a) Enter auf Location → Filter 'lama' --------------------------
        await typeQuery("checkmate")
        await typeQuery("checkmate")
        await asyncio.sleep(0.4)
        print("[a] nach Suche 'checkmate': rows=", await js("document.querySelectorAll('.row').length"),
              "sel0=", await js("document.querySelectorAll('.row')[0] ? document.querySelectorAll('.row')[0].innerText.split('\\n')[0] : ''"))
        await pressEnter()
        check("a1 Detail offen nach Enter", await js(detail_open))
        print("    Titel:", await js(detail_title), "| IPC:", await js("JSON.stringify(window.__calls||[])"))
        n_all = await js("document.querySelectorAll('.pane-detail .ptab').length")
        await shot("10-location-detail-all")
        await typeQuery("lama")
        await asyncio.sleep(0.4)
        grp = await js(caps)
        locs = await js(prlocs)
        check("a2 Filter 'lama' → nur Lama-Terminal-Gruppe",
              len(grp) == 1 and "lama" in grp[0].lower(), f"caps={grp}")
        check("a3 Lama-Preiszeilen (Lama Feed/Lama Extract)",
              all("Lama" in x for x in locs) and len(locs) == 2, f"locs={locs}")
        check("a4 Vor Filter: mehrere Gruppen", n_all > 1, f"n={n_all}")
        await shot("11-location-detail-filter-lama")

        # ---- (c1) Esc → zurück, Feld leer -----------------------------------
        await pressEsc()
        check("c1 Esc schließt Detail", not await js(detail_open))
        check("c2 Suchfeld leer nach Esc", (await js(input_val)) == "", f"val={await js(input_val)!r}")
        check("c3 Hauptsuche zeigt Ergebnisliste wieder", await js("document.querySelectorAll('.row').length") > 0)

        # ---- (b) Enter auf Laranite → Filter 'L1' ----------------------------
        await typeQuery("laranite")
        await typeQuery("laranite")
        await asyncio.sleep(0.4)
        await pressEnter()
        check("b1 Detail offen (Laranite)", await js(detail_open) and "Laranite" in (await js(detail_title)))
        await typeQuery("L1")
        await asyncio.sleep(0.4)
        locs = await js(prlocs)
        check("b2 Filter 'L1' → nur L1-Orte", bool(locs) and all("L1" in x for x in locs), f"locs={locs}")
        await shot("12-commodity-detail-filter-L1")
        # Filter leeren = alles zeigen
        await typeQuery("")
        await asyncio.sleep(0.4)
        locs_all = await js(prlocs)
        check("b3 Filter leer → alles zeigen", len(locs_all) > len(locs), f"all={len(locs_all)} filtered={len(locs)}")

        # ---- (c2) Esc → zurück, Feld leer -----------------------------------
        await pressEsc()
        check("c4 Esc schließt Detail (Commodity-Pfad)", not await js(detail_open))
        check("c5 Suchfeld leer nach Esc", (await js(input_val)) == "")

        print("\n== Ergebnisse ==")
        fails = [r for r in results if not r[1]]
        for label, okk, detail in results:
            print(("PASS " if okk else "FAIL ") + label + (" | " + detail if detail else ""))
        print("TOTAL:", len(results) - len(fails), "/", len(results), "PASS")
        if console_events:
            print("CONSOLE:", console_events[:10])
        proc.terminate()
        raise SystemExit(1 if fails else 0)

asyncio.run(main())
