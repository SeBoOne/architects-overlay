#!/usr/bin/env python3
"""CDP-Probe für QX (Inventar-Cards nach Material gruppieren).

Belegt auf der echten Svelte-Oberfläche (Tauri-IPC gestubbt):
  1. F3 öffnet den Inventar-Modus, Cards-Chip toggelt die Kartenansicht.
  2. Cards-View: Header je MATERIAL (Gold, Titanium) — nicht mehr Standort.
  3. Subgroups in der Karte = Standorte (Heim-Hangar / Meine Carrack /
     Org-Depot) mit Summen.
  4. Listenansicht (Cards aus) unverändert: weiterhin Standort-Header.
  5. Screenshot der Cards-Ansicht mit Material-Headern.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/inventory")))
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
        "--remote-debugging-port=9231", "--user-data-dir=/tmp/cdp-inv-cards-mat",
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

        VK = {"F1": 112, "F4": 115, "Escape": 27}
        async def key(k):
            vk = VK.get(k, 0)
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.3)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", OUT / name)

        async def smode():
            return (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()

        # ---- 1. F4 → Inventar (F3 = Routen seit Mode-Remap) ----
        await js("var i=document.querySelector('input.q'); if(i) i.focus();")
        await key("F4")
        check("f4-opens-inventory", await smode() == "INVENTAR", await smode())

        # ---- 2. Listenansicht (Baseline): Standort-Gruppen ----
        groups = json.loads(await js("JSON.stringify([...document.querySelectorAll('.vgroup')].map(function(e){return e.childNodes[0].textContent.trim();}))"))
        check("list-view-still-by-location", groups == ["Heim-Hangar", "Meine Carrack", "Org-Depot"], str(groups))

        # ---- 3. Cards-Chip → Kartenansicht nach MATERIAL ----
        await js("var c=[...document.querySelectorAll('.inv-bar .chip')].find(function(e){return e.innerText.trim().toLowerCase()==='cards';}); if(c) c.click(); true;")
        await asyncio.sleep(0.4)
        heads = json.loads(await js("JSON.stringify([...document.querySelectorAll('.inv-cards .vgroup')].map(function(e){return e.childNodes[0].textContent.trim();}))"))
        check("cards-headers-are-materials", heads == ["Gold", "Titanium"], str(heads))
        locsub = json.loads(await js("JSON.stringify([...document.querySelectorAll('.inv-cards .isub-btn')].map(function(e){return e.innerText.trim();}))"))
        check("cards-subgroups-are-locations", set(locsub) == {"Heim-Hangar", "Meine Carrack", "Org-Depot"}, str(locsub))
        foot = (await js("document.querySelector('.inv-foot') ? document.querySelector('.inv-foot').innerText : 'NONE'")).replace("\n", " · ")
        check("footer-unchanged", "3 Positionen" in foot and "2 Materialien" in foot, foot)
        await shot("qx-cards-by-material.png")

        # ---- 4. Header-Klick öffnet Material-Detail; Sub-Klick Standort-Detail.
        # Achtung: Esc aus einem Inventar-Detail exit den kompletten Modus
        # (VORBESTEHEND auf main, gleicher Befund ohne meinen Diff) — daher
        # nach jedem Esc F4-Reentry; cardView persisted via localStorage. ----
        await js("document.querySelector('.inv-cards .isub-btn').click();")
        await asyncio.sleep(0.4)
        det = await js("!!document.querySelector('.inv-detail')")
        check("sub-click-opens-location-detail", bool(det))
        await key("Escape")
        await asyncio.sleep(0.3)
        await key("F4")
        cards_back = await js("!!document.querySelector('.inv-cards')")
        check("reenter-keeps-cards-mode", bool(cards_back))
        await js("document.querySelector('.inv-cards .vgroup').click();")
        await asyncio.sleep(0.4)
        det2 = await js("!!document.querySelector('.inv-detail')")
        title2 = (await js("document.querySelector('.inv-detail .if-title') ? document.querySelector('.inv-detail .if-title').innerText : 'NONE'")).strip()
        check("header-click-opens-material-detail", bool(det2) and "GOLD" in str(title2).upper(), str(title2))
        await key("Escape")
        await asyncio.sleep(0.3)
        await key("F4")
        await asyncio.sleep(0.3)

        # ---- 5. Cards aus → Listenansicht unverändert ----
        await js("var c=[...document.querySelectorAll('.inv-bar .chip')].find(function(e){return e.innerText.trim().toLowerCase()==='cards';}); if(c) c.click(); true;")
        await asyncio.sleep(0.4)
        groups2 = json.loads(await js("JSON.stringify([...document.querySelectorAll('.vgroup')].map(function(e){return e.childNodes[0].textContent.trim();}))"))
        check("toggle-back-list-still-by-location", groups2 == ["Heim-Hangar", "Meine Carrack", "Org-Depot"], str(groups2))
        await key("Escape")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS, {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())
