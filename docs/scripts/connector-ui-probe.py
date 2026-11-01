#!/usr/bin/env python3
"""CDP-Probe für t_91124322 (C2: Connector-UI — Plugins-Settings, generische
Views, Placement own/replace/embed).

Datenquelle: STUBS (smoke-stub.js — __STUB_CONNECTORS__ + __STUB_CONNECTOR_DATA__).
Die echte Svelte-UI läuft unverändert; die IPC-Grenze ist gestubbt. Verifiziert:
  1. Boot: own-Connector in der Rail (Plug-Icon, Label, Hotkey) — F7 öffnet
     den eigenen Bereich (KEIN universeller „Dienste“-Bereich).
  2. Generische Views: Tabelle (Zeilen aus Mapping), Detail, Formular
     (Pflichtfeld-Hinweis, Submit → connector_form_submit, Erfolgs-Note).
  3. F12 → Plugins-Sektion: Connector gelistet (Placement-Typo, Toggle,
     Entfernen), Auth-Feld NUR bei header-Deklaration (maskiert), Hotkey-
     Capture-Feld für own.
  4. replace: aktivierter Hangar-Connector ersetzt die Hangar-Sektion (Chip),
     Deaktivieren holt die lokale Liste zurück (Zeilen > 0).
  5. embed: Connector-Views erscheinen als Unter-Sektion im Inventar.
Screenshots: 01-own-table, 02-own-detail, 03-own-form, 04-plugins,
05-replace-hangar, 06-embed-inventory.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/connector-ui")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

OWN = {
    "id": "example-own", "enabled": True,
    "auth_set": {"example.key": False}, "hotkey": "F7",
    "manifest": {
        "manifest_version": 1, "id": "example-own",
        "name": {"de": "Beispiel-Dienst", "en": "Example Service"},
        "base_url": "https://api.example.com/v1",
        "auth": {"type": "header", "header": "X-Example-Key", "value_source": "example.key"},
        "placement": {"kind": "own", "label": {"de": "Beispiel", "en": "Example"}, "hotkey_slot": True},
        "views": [
            {"id": "ships", "type": "table", "title": {"de": "Schiffe", "en": "Ships"},
             "endpoint": "/ships", "mapping": {"name": "data[].ship_name", "owner": "data[].owner"},
             "poll_seconds": 300},
            {"id": "status", "type": "detail", "title": {"de": "Status", "en": "Status"},
             "endpoint": "/status", "mapping": {"name": "service.name", "version": "service.version"}},
            {"id": "create", "type": "form", "title": {"de": "Anlegen", "en": "Create"},
             "endpoint": "/things",
             "form": {"fields": [
                 {"key": "name", "label": {"de": "Name", "en": "Name"}, "type": "text", "required": True}],
                 "method": "post", "path": "/things", "body_template": {"name": "{name}"}}}
        ]
    }
}
REPLACE = {
    "id": "example-replace", "enabled": False, "auth_set": {}, "hotkey": None,
    "manifest": {
        "manifest_version": 1, "id": "example-replace",
        "name": {"de": "Externer Hangar", "en": "External Hangar"},
        "base_url": "https://api.example.com/v1", "placement": {"kind": "replace", "target": "hangar"},
        "views": [{"id": "fleet", "type": "table", "title": {"de": "Flotte", "en": "Fleet"},
                   "endpoint": "/fleet", "mapping": {"name": "data[].ship_name"}}]
    }
}
EMBED = {
    "id": "example-embed", "enabled": False, "auth_set": {}, "hotkey": None,
    "manifest": {
        "manifest_version": 1, "id": "example-embed",
        "name": {"de": "Lager-Dienst", "en": "Depot Service"},
        "base_url": "https://api.example.com/v1", "placement": {"kind": "embed", "target": "inventory"},
        "views": [{"id": "boxes", "type": "cards", "title": {"de": "Kisten", "en": "Boxes"},
                   "endpoint": "/boxes", "mapping": {"name": "data[].name", "count": "data[].count"}}]
    }
}
DATA = {
    "ships": {"rows": [
        {"name": "Sabre", "owner": "Sebo"},
        {"name": "Kraken", "owner": "Sebo"}]},
    "status": {"mapped": {"name": "example-service", "version": "1.2.3"}},
    "create": {"rows": []},
    "fleet": {"rows": [{"name": "Ext-Ship-1"}, {"name": "Ext-Ship-2"}]},
    "boxes": {"rows": [{"name": "Titanium", "count": 12}, {"name": "Agricium", "count": 3}]}
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
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-connector",
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
                    + "\nwindow.__STUB_CONNECTORS__ = " + json.dumps([OWN, REPLACE, EMBED]) + ";"
                    + "\nwindow.__STUB_CONNECTOR_DATA__ = " + json.dumps(DATA) + ";"
                    + "\nwindow.__HANGAR_DATA__ = " + json.dumps([
                        {"id": 1, "name": "Sabre", "origin": "pledge", "insurance": "LTI",
                         "custom_name": "", "entity_type": "ship", "base_ship": "", "ship_code": "",
                         "manufacturer": "", "purchase_price": None, "purchase_date": "",
                         "pledge_id": "", "pledge_name": "", "warbond": False, "ccud": False,
                         "created_at": None, "updated_at": None},
                        {"id": 2, "name": "Cutter", "origin": "ingame", "insurance": None,
                         "custom_name": "", "entity_type": "ship", "base_ship": "", "ship_code": "",
                         "manufacturer": "", "purchase_price": None, "purchase_date": "",
                         "pledge_id": "", "pledge_name": "", "warbond": False, "ccud": False,
                         "created_at": None, "updated_at": None}]) + ";")
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": boot})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate",
                                  {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + json.dumps(r["exceptionDetails"].get("exception", {}).get("description", ""))[:300]
                return r.get("result", {}).get("value")

            VK = {"F7": 118, "F11": 122, "F12": 123, "F4": 115, "Escape": 27}
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

            # ---- 1) own-Placement: Rail-Eintrag + F7 öffnet den eigenen Bereich
            rail_own = await js("[...document.querySelectorAll('.rail .rentry .rlbl')].map(e=>e.innerText)")
            check("rail-own-entry", any("Example Service" in (r or "") for r in (rail_own or [])), str(rail_own))
            await key("F7"); await asyncio.sleep(0.6)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()
            check("f7-opens-own", "EXAMPLE SERVICE" in smode.upper(), smode)
            tabs = await js("[...document.querySelectorAll('.connector .ctab')].map(e=>e.innerText)")
            check("own-view-tabs", tabs and len(tabs) == 3, str(tabs))
            rows = int(await js("document.querySelectorAll('.connector .ctable tbody tr').length"))
            check("table-rows", rows == 2, "rows=" + str(rows))
            await shot("01-own-table.png")

            # Detail-Tab
            await js("[...document.querySelectorAll('.connector .ctab')].find(b=>b.innerText.trim()==='Status').click()")
            await asyncio.sleep(0.4)
            detail = await js("document.querySelector('.connector .cdetail') ? 'yes' : 'no'")
            check("detail-view", detail == "yes", detail)
            await shot("02-own-detail.png")

            # Formular-Tab: Pflichtfeld + Submit
            await js("[...document.querySelectorAll('.connector .ctab')].find(b=>b.innerText.trim()==='Create').click()")
            await asyncio.sleep(0.4)
            form = await js("document.querySelector('.connector form.cform') ? 'yes' : 'no'")
            check("form-view", form == "yes", form)
            await js("document.querySelector('.connector form.cform button[type=submit]').click()")
            await asyncio.sleep(0.4)
            note = (await js("document.querySelector('.cform-note') ? document.querySelector('.cform-note').innerText : ''")).strip()
            check("form-required-note", "required" in note.lower() or "erforderlich" in note.lower(), note)
            await js("var i=document.querySelector('.connector form.cform input[type=text]');"
                     "i.value='Test-Thing';i.dispatchEvent(new Event('input',{bubbles:true}));")
            await js("document.querySelector('.connector form.cform button[type=submit]').click()")
            await asyncio.sleep(0.4)
            note2 = (await js("document.querySelector('.cform-note') ? document.querySelector('.cform-note').innerText : ''")).strip()
            check("form-submit-ok", "201" in note2 or "Saved" in note2 or "Gespeichert" in note2, note2)
            await shot("03-own-form.png")

            # ---- 3) F12 → Plugins-Sektion
            await key("F12"); await asyncio.sleep(0.6)
            sect = await js("document.querySelector('.cfg-sect-plugins') ? 'yes' : 'no'")
            check("plugins-section-visible", sect == "yes", sect)
            plabel = (await js("document.querySelector('.cfg-sect-plugins .h') ? document.querySelector('.cfg-sect-plugins .h').innerText : 'NONE'")).strip()
            check("plugins-title", plabel.upper() == "PLUGINS", plabel)
            prows = int(await js("document.querySelectorAll('.cfg-plugin-row').length"))
            check("plugins-list-3", prows == 3, "rows=" + str(prows))
            ptype = await js("document.querySelector('.cfg-sect-plugins .cfg-note') ? document.querySelector('.cfg-sect-plugins .cfg-note').innerText : 'NONE'")
            check("placement-typo-shown", "Own" in ptype or "top-level" in ptype, ptype)
            # Auth-Feld nur beim own-Connector (header-Deklaration): 1 password-Feld
            pw = int(await js("document.querySelectorAll('.cfg-sect-plugins input[type=password]').length"))
            check("auth-field-only-declared", pw == 1, "pw=" + str(pw))
            hotkeys = int(await js("document.querySelectorAll('.cfg-sect-plugins .cfg-key-row').length"))
            check("hotkey-capture-present", hotkeys >= 1, "rows=" + str(hotkeys))
            await shot("04-plugins.png")

            # Toggle own off → Rail-Eintrag weg, F7 wirkt nicht mehr
            await js("document.querySelector('.cfg-sect-plugins .cfg-plugin-row .cfg-chips .chip').click()")
            await asyncio.sleep(0.5)
            rail_own2 = await js("[...document.querySelectorAll('.rail .rentry .rlbl')].map(e=>e.innerText)")
            check("toggle-hides-rail-entry", not any("Example Service" in (r or "") for r in (rail_own2 or [])), str(rail_own2))
            await js("document.querySelector('.cfg-sect-plugins .cfg-plugin-row .cfg-chips .chip').click()")
            await asyncio.sleep(0.4)
            rail_own3 = await js("[...document.querySelectorAll('.rail .rentry .rlbl')].map(e=>e.innerText)")
            check("toggle-restores-rail-entry", any("Example Service" in (r or "") for r in (rail_own3 or [])), str(rail_own3))
            await key("Escape"); await asyncio.sleep(0.4)

            # ---- 4) replace: Hangar-Connector aktivieren → F11 zeigt Views + Chip
            await key("F12"); await asyncio.sleep(0.5)
            enabled_off = await js("[...document.querySelectorAll('.cfg-sect-plugins .cfg-plugin-row')]"
                                   ".map(r=>r.querySelector('.cfg-chips .chip').innerText)")
            # zweiter Connector (example-replace) aktivieren
            await js("[...document.querySelectorAll('.cfg-sect-plugins .cfg-plugin-row')][1]"
                     ".querySelector('.cfg-chips .chip').click()")
            await asyncio.sleep(0.5)
            await key("F11"); await asyncio.sleep(0.6)
            chip = await js("document.querySelector('.hangar-note-chip') ? document.querySelector('.hangar-note-chip').innerText : 'NONE'")
            check("replace-chip", chip != "NONE", chip)
            frows = int(await js("document.querySelectorAll('.connector .ctable tbody tr').length"))
            check("replace-table-rows", frows == 2, "rows=" + str(frows))
            local = await js("document.querySelectorAll('.hangar-list .hrow').length")
            check("replace-hides-local-hangar", local == 0, "local-rows=" + str(local))
            await shot("05-replace-hangar.png")
            # Deaktivieren → lokale Hangar-Liste kommt zurück
            await key("F12"); await asyncio.sleep(0.5)
            await js("[...document.querySelectorAll('.cfg-sect-plugins .cfg-plugin-row')][1]"
                     ".querySelector('.cfg-chips .chip').click()")
            await asyncio.sleep(0.5)
            await key("F11"); await asyncio.sleep(0.8)
            lrows = int(await js("document.querySelectorAll('.hangar-list .hrow').length"))
            check("disable-restores-local-hangar", lrows > 0, "rows=" + str(lrows))
            await key("Escape"); await asyncio.sleep(0.4)

            # ---- 5) embed: Inventar-Connector aktivieren → Unter-Sektion in F4
            await key("F12"); await asyncio.sleep(0.5)
            await js("[...document.querySelectorAll('.cfg-sect-plugins .cfg-plugin-row')][2]"
                     ".querySelector('.cfg-chips .chip').click()")
            await asyncio.sleep(0.5)
            await key("F4"); await asyncio.sleep(0.8)
            embedded = await js("document.querySelector('.connector.is-embedded') ? 'yes' : 'no'")
            check("embed-in-inventory", embedded == "yes", embedded)
            cards = int(await js("document.querySelectorAll('.connector.is-embedded .ccard').length"))
            check("embed-cards", cards == 2, "cards=" + str(cards))
            await shot("06-embed-inventory.png")
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
