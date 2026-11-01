#!/usr/bin/env python3
"""CDP-Probe für t_d5ca9277 (C3: SCLM-Pilot-Connector — Manifest + Formular-
Views live).

Datenquelle: STUBS (smoke-stub.js — __STUB_CONNECTORS__ + __STUB_CONNECTOR_DATA__).
Die echte Svelte-UI läuft unverändert; die IPC-Grenze ist gestubbt. Das
Stub-Connectoren-Array ist die im Repo gepflegte SCLM-Manifest-Struktur
(examples/sclm.json, Placement kind=own + hotkey_slot — Sebo-Korrektur:
eigener Bereich statt Dienste-Tab). Verifiziert:

  1. Boot: SCLM in der Rail (own-Placement, F7 öffnet den eigenen Bereich).
  2. Sessions-Tabelle rendert Zeilen aus dem Root-Array-Mapping.
  3. Session-Detail mit `{param}`-Endpoint: ohne Code → param_required-Hinweis
     (KEIN Fetch), mit Code → Fetch mit params (Stub-Aufzeichnung), Pin
     speichert in localStorage + überlebt Tab-Wechsel, Unpin räumt auf.
  4. Beitreten-Formular: Name-Feld (default_source uex_username) ist NICHT
     required — Submit geht mit leerem Namen durch (Backend füllt den
     Standard-Benutzernamen), Werte landen im Submit-Aufruf.
  5. Event-erstellen-Formular: Pflichtfeld-Block + Erfolg mit Antwort-Körper
     (SCLM liefert admin_code/join_code — generisch im Formular-Note).
Screenshots: 01-sclm-table, 02-sclm-detail-param, 03-sclm-detail-pinned,
04-sclm-join-form, 05-sclm-create-ok.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
MANIFEST = json.loads((ROOT / "examples/sclm.json").read_text())
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/connector-sclm")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

SCLM = {
    "id": "sclm", "enabled": True, "auth_set": {}, "hotkey": "F7",
    "manifest": MANIFEST
}
DATA = {
    "sessions": {"rows": [
        {"name": "Weihnachtsverlosung", "created": "01.11.2026", "participants": 7,
         "loot": 12, "join_code": "AB12CD", "raffle_done": False},
        {"name": "AAWA Monday Raffle", "created": "02.11.2026", "participants": 4,
         "loot": 6, "join_code": "WX34YZ", "raffle_done": True}]},
    "session_detail": {"mapped": {
        "name": "Weihnachtsverlosung", "created": "01.11.2026", "max_wishes": 3,
        "locked": False, "raffle_done": False, "raffle_mode": "wish",
        "participants": ["Sebo", "Debi", "Richy"],
        "loot": ["Sabre", "Cutlass Black", "Kraken"],
        "winners": []}},
    "raffle_result": {"mapped": {
        "participant": "Sebo", "won": ["Cutlass Black"], "by_wish": True,
        "waived": False, "collected": False}},
}

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8797", "--bind", "127.0.0.1",
        "--directory", str(ROOT / "build"),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    for _ in range(60):
        try:
            with urllib.request.urlopen(URL, timeout=1) as r:
                if r.status == 200: break
        except Exception: pass
        await asyncio.sleep(0.25)
    else:
        raise RuntimeError("static server did not come up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-connector-sclm",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9234/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception: pass
        await asyncio.sleep(0.25)
    if ws is None:
        server.terminate(); proc.terminate()
        raise RuntimeError("chromium CDP did not come up")

    try:
        async with websockets.connect(ws, max_size=None) as cdp:
            msg_id = 0; pending = {}
            async def call(method, params=None):
                nonlocal msg_id
                msg_id += 1; mid = msg_id
                fut = asyncio.get_event_loop().create_future(); pending[mid] = fut
                await cdp.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
                return await fut
            async def recv_loop():
                async for raw in cdp:
                    msg = json.loads(raw)
                    if "id" in msg and msg["id"] in pending:
                        pending[msg["id"]].set_result(msg)
            asyncio.create_task(recv_loop())
            def ok(resp):
                if "error" in resp: raise RuntimeError("CDP error: %s" % resp["error"])
                return resp.get("result", {})

            await call("Page.enable"); await call("Runtime.enable")
            await call("Emulation.setDeviceMetricsOverride",
                       {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
            boot = (STUB
                    + "\nwindow.__STUB_CONNECTORS__ = " + json.dumps([SCLM]) + ";"
                    + "\nwindow.__STUB_CONNECTOR_DATA__ = " + json.dumps(DATA) + ";"
                    + "\nwindow.__STUB_CONNECTOR_FORM_BODY__ = "
                    + json.dumps({"id": "abc123xy", "admin_code": "ADM9QK", "join_code": "JOIN7Z"}) + ";")
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": boot})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate",
                                  {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + json.dumps(r["exceptionDetails"].get("exception", {}).get("description", ""))[:300]
                return r.get("result", {}).get("value")

            VK = {"F7": 118, "Escape": 27}
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

            def tab(name):
                return (f"[...document.querySelectorAll('.connector .ctab')]"
                        f".find(b=>b.innerText.trim()==='{name}')")

            # ---- 1) own-Placement: SCLM in der Rail, F7 öffnet den Bereich
            rail = await js("[...document.querySelectorAll('.rail .rentry .rlbl')].map(e=>e.innerText)")
            check("rail-sclm-entry", any("SCLM" in (r or "") for r in (rail or [])), str(rail))
            await key("F7"); await asyncio.sleep(0.6)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()
            check("f7-opens-sclm", "SCLM" in smode.upper(), smode)
            tabs = await js("[...document.querySelectorAll('.connector .ctab')].map(e=>e.innerText)")
            check("sclm-view-tabs", tabs and len(tabs) == 10, str(tabs))

            # ---- 2) Sessions-Tabelle (Root-Array-Mapping)
            rows = int(await js("document.querySelectorAll('.connector .ctable tbody tr').length"))
            check("sessions-table-rows", rows == 2, "rows=" + str(rows))
            text = await js("document.querySelector('.connector .ctable') ? document.querySelector('.connector .ctable').innerText : ''")
            check("sessions-table-data", "Weihnachtsverlosung" in text and "AAWA Monday Raffle" in text, text[:200])
            await shot("01-sclm-table.png")

            # ---- 3) Session-Detail: Param-Prompt → Fetch mit Params → Pin
            await js(tab("Session Detail") + ".click()"); await asyncio.sleep(0.5)
            err = (await js("document.querySelector('.cerror') ? document.querySelector('.cerror').innerText : ''")).strip()
            check("detail-param-required-hint", "required" in err.lower() or "benötigt" in err, err)
            last = await js("window.__STUB_LAST_FETCH__ ? window.__STUB_LAST_FETCH__.viewId : null")
            check("detail-no-fetch-without-param", last == "sessions", str(last))
            await shot("02-sclm-detail-param.png")
            await js("var i=document.querySelector('.cparams input');"
                     "i.value='AB12CD';i.dispatchEvent(new Event('input',{bubbles:true}));"
                     "i.dispatchEvent(new Event('change',{bubbles:true}));")
            await asyncio.sleep(0.6)
            detail = await js("document.querySelector('.connector .cdetail') ? document.querySelector('.connector .cdetail').innerText : ''")
            check("detail-renders-with-param", "Weihnachtsverlosung" in detail and "Sebo" in detail, detail[:200])
            lastp = await js("window.__STUB_LAST_FETCH__ ? JSON.stringify(window.__STUB_LAST_FETCH__.params) : null")
            check("fetch-sends-params", lastp is not None and "AB12CD" in (lastp or ""), str(lastp))
            # Pin speichert (localStorage, Connector-Zustand — NICHT Settings)
            await js("document.querySelector('.cparams .btn-ghost').click()"); await asyncio.sleep(0.3)
            pinned = await js("localStorage.getItem('connector.pin.sclm.session_detail')")
            check("pin-saved-localstorage", pinned is not None and "AB12CD" in (pinned or ""), str(pinned))
            await shot("03-sclm-detail-pinned.png")
            # Tab weg + zurück → Pin wiederhergestellt
            await js(tab("Raffles") + ".click()"); await asyncio.sleep(0.4)
            await js(tab("Session Detail") + ".click()"); await asyncio.sleep(0.5)
            val = await js("document.querySelector('.cparams input') ? document.querySelector('.cparams input').value : ''")
            check("pin-survives-tab-switch", val == "AB12CD", val)
            # Unpin räumt auf
            await js("document.querySelector('.cparams .btn-ghost').click()"); await asyncio.sleep(0.3)
            pinned2 = await js("localStorage.getItem('connector.pin.sclm.session_detail')")
            check("unpin-clears", pinned2 in (None, "", "{}"), str(pinned2))

            # ---- 4) Beitreten: Name-Feld NICHT required (default_source)
            await js(tab("Join") + ".click()"); await asyncio.sleep(0.4)
            labels = await js("[...document.querySelectorAll('.cform .cfield-label')].map(e=>e.innerText)")
            check("join-has-name-hint", any("default username" in (l or "") for l in (labels or [])), str(labels))
            await js("document.querySelector('.cform input')?.focus()")
            await js("var f=document.querySelector('.cform');"
                     "var inp=[...f.querySelectorAll('input')];"
                     "inp[0].value='abc123xy';inp[0].dispatchEvent(new Event('input',{bubbles:true}));"
                     "inp[1].value='JOIN7Z';inp[1].dispatchEvent(new Event('input',{bubbles:true}));")
            await js("document.querySelector('.cform button[type=submit]').click()")
            await asyncio.sleep(0.5)
            submit = await js("window.__STUB_LAST_SUBMIT__ ? JSON.stringify(window.__STUB_LAST_SUBMIT__) : null")
            check("join-submit-values", submit is not None and '"code":"JOIN7Z"' in submit.replace("'", '"'), submit)
            note = (await js("document.querySelector('.cform-note') ? document.querySelector('.cform-note').innerText : ''")).strip()
            check("join-ok-note", "201" in note or "Saved" in note or "Gespeichert" in note, note)
            await shot("04-sclm-join-form.png")

            # ---- 5) Event erstellen: Pflichtfeld + Antwort-Körper im Note
            await js(tab("Create Event") + ".click()"); await asyncio.sleep(0.4)
            await js("document.querySelector('.cform button[type=submit]').click()")
            await asyncio.sleep(0.4)
            note = (await js("document.querySelector('.cform-note') ? document.querySelector('.cform-note').innerText : ''")).strip()
            check("create-required-note", "required" in note.lower() or "erforderlich" in note.lower(), note)
            await js("var f=document.querySelector('.cform');"
                     "var inp=[...f.querySelectorAll('input')];"
                     "inp[0].value='Probe-Event';inp[0].dispatchEvent(new Event('input',{bubbles:true}));"
                     "inp[1].value='3';inp[1].dispatchEvent(new Event('input',{bubbles:true}));")
            await js("document.querySelector('.cform button[type=submit]').click()")
            await asyncio.sleep(0.5)
            note2 = (await js("document.querySelector('.cform-note') ? document.querySelector('.cform-note').innerText : ''")).strip()
            check("create-ok-with-codes-body", "ADM9QK" in note2 and "JOIN7Z" in note2, note2)
            await shot("05-sclm-create-ok.png")
    finally:
        for p_ in (server, proc):
            try:
                p_.terminate()
            except ProcessLookupError:
                pass

    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit("FAILED: " + ", ".join(FAIL))

asyncio.run(main())
