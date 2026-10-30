#!/usr/bin/env python3
"""CDP-Probe für t_988e132f (S3: SC-Wiki-Modul-Kompatibilität).

IPC-Grenze gestubbt (smoke-stub.js, window.__MODUL_*__), echte Svelte-UI:

  1. F11 → Hangar, Detail Aurora Mk II (Zuordnung bekannt, Ports) →
     Modul-Autocomplete schlägt NUR die passenden Module vor
     (Aurora Mk II DM/TS Module) — Katalog-Fremdmodule (Apollo) NICHT.
  2. Detail Hull E (Zuordnung: ohne Ports) → Modul-Eingabefeld versteckt
     (+ Hinweis-Zeile).
  3. F12 → Settings-Section „Modul-Kompatibilität (SC-Wiki)" mit Version +
     Alter; Button-Klick (stub refresh) setzt die Bestätigungsnote.

Screenshots: 01-aurora-modules.png, 02-noport-hidden.png, 03-settings.png.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/scwiki-modules")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8796/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

DM = "Aurora Mk II DM Module"
TS = "Aurora Mk II TS Module"
APOLLO = "Apollo Tier 1 Module Left"
SHIELD = "5CA 'Akura'"

CATALOG = [
    {"name": DM, "category": "Modul"}, {"name": TS, "category": "Modul"},
    {"name": APOLLO, "category": "Modul"}, {"name": SHIELD, "category": "Schild"},
    {"name": "Bolide", "category": "Generator"},
]

HANGAR = [
    {"id": 1, "name": "Aurora Mk II", "origin": "pledge", "insurance": "LTI",
     "custom_name": "", "entity_type": "ship", "base_ship": "", "ship_code": "",
     "manufacturer": "Roberts Space Industries", "purchase_price": None,
     "purchase_date": "", "pledge_id": "", "pledge_name": "", "warbond": 0,
     "ccud": 0, "created_at": 1700000000, "updated_at": 1700000000},
    {"id": 2, "name": "Hull E", "origin": "ingame", "insurance": None,
     "custom_name": "", "entity_type": "ship", "base_ship": "", "ship_code": "",
     "manufacturer": "MISC", "purchase_price": None,
     "purchase_date": "", "pledge_id": "", "pledge_name": "", "warbond": 0,
     "ccud": 0, "created_at": 1700000000, "updated_at": 1700000000},
]

MODUL_STATUS = {
    "version": "4.10.2-LIVE.12881860", "synced_at": 1760000000,
    "age_seconds": 5400, "module_count": 1204, "ship_count": 268,
}
FOR_SHIP = {
    "aurora mk ii": {"known": True, "has_ports": True,
                      "modules": [{"name": DM, "category": "Modul"},
                                   {"name": TS, "category": "Modul"}]},
    "hull e": {"known": True, "has_ports": False, "modules": []},
}

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
        "--remote-debugging-port=9235", "--user-data-dir=/tmp/cdp-scwiki-s3",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9235/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        server.terminate(); proc.terminate()
        raise RuntimeError("CDP did not come up")

    try:
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
            prelude = (STUB
                + "\nwindow.__HANGAR_DATA__ = " + json.dumps(HANGAR) + ";"
                + "\nwindow.__MODUL_STATUS__ = " + json.dumps(MODUL_STATUS) + ";"
                + "\nwindow.__MODUL_ALL__ = " + json.dumps(CATALOG) + ";"
                + "\nwindow.__MODUL_FOR_SHIP__ = " + json.dumps(FOR_SHIP) + ";")
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": prelude})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + (r["exceptionDetails"].get("text") or "")
                return r.get("result", {}).get("value")

            VK = {"F11": 122, "F12": 123, "Escape": 27, "Enter": 13, "ArrowDown": 40}
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

            # ---- 1: Aurora Mk II — Autocomplete zeigt NUR passende Module ----
            await key("F11")
            await asyncio.sleep(1.0)
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('Aurora Mk II')).click()")
            await asyncio.sleep(1.0)
            check("aurora-detail-open",
                  (await js("document.querySelector('.hangar-detail') ? 'yes' : 'no'")) == "yes")
            check("aurora-module-section",
                  (await js("(() => { const d=document.querySelector('.hangar-detail .hd-mods .h'); return d ? d.innerText : 'NONE'; })()")) .startswith("Module"))
            check("aurora-module-field-visible",
                  (await js("document.querySelector('.hangar-detail .hmod-add') ? 'yes' : 'no'")) == "yes")
            # Typeahead: leer abfragen → passende Module zuerst
            await js("(() => { const i = document.querySelector('.hangar-detail .hmod-add .rta-input'); i.focus(); i.value=''; i.dispatchEvent(new Event('input', {bubbles:true})); })()")
            await asyncio.sleep(0.5)
            opts = await js("(() => { const d = document.querySelector('.hangar-detail .hmod-add .rta-input');"
                            " const dd = d.closest('.hmod-add').querySelector('.rta-dd, .rta-dropdown, .rta-menu, [class*=dropdown], [class*=drop]');"
                            " return dd ? dd.innerText : 'NODROP'; })()")
            check("aurora-suggest-dm-ts", DM in str(opts) and TS in str(opts), str(opts)[:200])
            check("aurora-suggest-not-apollo", APOLLO not in str(opts), str(opts)[:200])
            # Query 'dm' filtert auf das DM-Modul
            await js("(() => { const i = document.querySelector('.hangar-detail .hmod-add .rta-input'); i.focus(); i.value='dm'; i.dispatchEvent(new Event('input', {bubbles:true})); })()")
            await asyncio.sleep(0.5)
            opts2 = await js("(() => { const d = document.querySelector('.hangar-detail .hmod-add .rta-input');"
                             " const dd = d.closest('.hmod-add').querySelector('.rta-dd, .rta-dropdown, .rta-menu, [class*=dropdown], [class*=drop]');"
                             " return dd ? dd.innerText : 'NODROP'; })()")
            check("aurora-query-dm-filters", DM in str(opts2) and TS not in str(opts2), str(opts2)[:200])
            await shot("01-aurora-modules.png")
            # Deterministische Neu-Navigation: Reload (Stub via
            # AddScriptToEvaluateOnNewDocument neu injiziert), F11 → Hangar,
            # dann direkt Hull E. Escape/F11-Rückweg aus dem Detail ist im
            # Live-DOM nicht reproduzierbar dispatch-bar (Statusbar-Taste).
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)
            await key("F11")
            await asyncio.sleep(1.2)
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('Hull E'))?.click()")
            await asyncio.sleep(1.2)

            # ---- 2: Hull E (ohne Ports) — Modulfeld versteckt ----
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('Hull E')).click()")
            await asyncio.sleep(1.2)
            check("hull-detail-open",
                  (await js("document.querySelector('.hangar-detail') ? 'yes' : 'no'")) == "yes")
            check("hull-module-field-hidden",
                  (await js("document.querySelector('.hangar-detail .hmod-add') ? 'VISIBLE' : 'hidden'")) == "hidden")
            check("hull-noport-note",
                  "keine Modul-Ports" in str(await js("document.querySelector('.hangar-detail .hd-mods') ? document.querySelector('.hangar-detail .hd-mods').innerText : 'NONE'")))
            await shot("02-noport-hidden.png")

            # ---- 3: Settings — Section + Button ----
            await key("F12")
            await asyncio.sleep(1.5)
            sec = await js("(() => { const h=[...document.querySelectorAll('.cfg-sect .h')].find(x=>x.innerText.toUpperCase().includes('MODUL-KOMPATIBILITÄT')); return h ? h.closest('.cfg-sect').innerText : 'NONE'; })()")
            check("settings-section-present", "MODUL-KOMPATIBILITÄT" in str(sec).upper(), str(sec)[:150])
            check("settings-version-shown", "4.10.2-LIVE.12881860" in str(sec))
            check("settings-age-shown", "vor " in str(sec))
            await js("(() => { const s=[...document.querySelectorAll('.cfg-sect')].find(x=>x.innerText.toUpperCase().includes('MODUL-KOMPATIBILITÄT'));"
                     " [...s.querySelectorAll('button')].find(b=>b.innerText.toUpperCase().includes('AKTUALISIEREN')).click(); })()")
            await asyncio.sleep(0.5)
            note = await js("(() => { const s=[...document.querySelectorAll('.cfg-sect')].find(x=>x.innerText.toUpperCase().includes('MODUL-KOMPATIBILITÄT'));"
                            " const n=s.querySelector('.cfg-note'); return n ? n.innerText : 'NONE'; })()")
            check("settings-refresh-note", "aktualisiert" in str(note), str(note))
            # Section in den Viewport scrollen, damit Screenshot 03 sie zeigt.
            await js("(() => { const s=[...document.querySelectorAll('.cfg-sect')].find(x=>x.innerText.toUpperCase().includes('MODUL-KOMPATIBILITÄT')); s.scrollIntoView({block:'center'}); return 'OK'; })()")
            await asyncio.sleep(0.4)
            await shot("03-settings.png")
    finally:
        server.terminate()
        proc.terminate()

    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())
