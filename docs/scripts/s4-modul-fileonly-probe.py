#!/usr/bin/env python3
"""CDP-Probe für t_66980b3c (S4: Modul-Zuweisung AUSSCHLIESSLICH aus Datei).

IPC-Grenze gestubbt (smoke-stub.js, window.__MODUL_*__), echte Svelte-UI.
Die Modul-Zuordnung (FOR_SHIP) kommt NICHT hartkodiert, sondern aus dem
Live-Test `live_sync_file_only_and_decision` (docs/scripts/s4-modul-fixture.json
— echte Gitea-Raw/Repo-Baseline + reale modul_for_ship_decision-Logik):

  1. Detail Aegis Eclipse (Datei-Eintrag) → Modul-Feld sichtbar,
     Autocomplete = NUR die Datei-Racks (S10/S3 Bomb Rack Weapon Kit),
     keine Fremd-Module.
  2. Detail Aegis Retaliator → genau die 11 Datei-Module (Query 'bomb'
     zeigt nur die 4 Racks, kein Galaxy/Torpedo-Fremdanteil).
  3. Detail RSI Galaxy → die 3 Datei-Module (Cargo/Med Bay/Refinery).
  4. Detail Aegis Arrow (KEIN Datei-Eintrag) → GAR KEIN Modul-Feld
     (S4 Nur-Datei-Regel) + Hinweis-Zeile.
  5. F12 → Settings-Section „Modul-Zuweisungen“, Quelle module-overrides.json,
     Button „Modul-Zuweisungen aktualisieren“, Klick setzt die Note.

Screenshots: 01-eclipse-racks.png, 02-retaliator-11.png, 03-galaxy-3.png,
04-arrow-no-field.png, 05-settings.png.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/scwiki-modules")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8796/"

# FOR_SHIP kommt aus dem REALEN Backend-Pfad (S4): docs/scripts/s4-modul-fixture.json
# wird vom Live-Test `live_sync_file_only_and_decision` erzeugt (echte
# Gitea-Raw/Repo-Baseline + reale modul_for_ship_decision-Logik) — die UI-Probe
# erfindet hier NICHTS.
FIXTURE_PATH = ROOT / "docs/scripts/s4-modul-fixture.json"
FIXTURE = json.loads(FIXTURE_PATH.read_text())
FOR_SHIP = FIXTURE["for_ship"]
ARROW = FOR_SHIP["aegis arrow"]
assert ARROW["known"] is False and ARROW["has_ports"] is False and ARROW["modules"] == [], \
    "Fixture ohne realen Arrow-No-Entry-Nachweis — erst Live-Test laufen lassen"
RACKS = {"S10 Bomb Rack Weapon Kit", "S3 Bomb Rack Weapon Kit"}
assert {m["name"] for m in FOR_SHIP["aegis eclipse"]["modules"]} == RACKS, \
    "Fixture ohne die Eclipse-Racks — erst Live-Test laufen lassen"
assert len(FOR_SHIP["aegis retaliator"]["modules"]) == 11, "Retaliator ≠ 11 Module"
assert len(FOR_SHIP["rsi galaxy"]["modules"]) == 3, "Galaxy ≠ 3 Module"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

def hangar_entry(id_, name, manufacturer):
    return {"id": id_, "name": name, "origin": "pledge", "insurance": "LTI",
            "custom_name": "", "entity_type": "ship", "base_ship": "", "ship_code": "",
            "manufacturer": manufacturer, "purchase_price": None,
            "purchase_date": "", "pledge_id": "", "pledge_name": "", "warbond": 0,
            "ccud": 0, "created_at": 1700000000, "updated_at": 1700000000}

HANGAR = [
    hangar_entry(1, "Aegis Eclipse", "Aegis Dynamics"),
    hangar_entry(2, "Aegis Retaliator", "Aegis Dynamics"),
    hangar_entry(3, "RSI Galaxy", "Roberts Space Industries"),
    hangar_entry(4, "Aegis Arrow", "Aegis Dynamics"),
]

MODUL_STATUS = {
    "version": "module-overrides.json", "synced_at": 1760000000,
    "age_seconds": 5400, "module_count": 36, "ship_count": 17,
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
        "--remote-debugging-port=9235", "--user-data-dir=/tmp/cdp-scwiki-s4",
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
                + "\nwindow.__MODUL_FOR_SHIP__ = " + json.dumps(FOR_SHIP) + ";")
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": prelude})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + (r["exceptionDetails"].get("text") or "")
                return r.get("result", {}).get("value")

            VK = {"F11": 122, "F12": 123, "Escape": 27, "Enter": 13}
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

            async def open_detail(name):
                # Deterministische Neu-Navigation: Reload (Stub wird via
                # AddScriptToEvaluateOnNewDocument neu injiziert), F11 → Hangar.
                await call("Page.navigate", {"url": URL})
                await asyncio.sleep(2.5)
                await key("F11")
                await asyncio.sleep(1.2)
                await js(f"[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes({json.dumps(name)})).click()")
                await asyncio.sleep(1.2)
                check(f"{name}-detail-open",
                      (await js("document.querySelector('.hangar-detail') ? 'yes' : 'no'")) == "yes")

            async def typeahead_options(query):
                await js("(() => { const i = document.querySelector('.hangar-detail .hmod-add .rta-input');"
                         " i.focus(); i.value=" + json.dumps(query) + "; i.dispatchEvent(new Event('input', {bubbles:true})); })()")
                await asyncio.sleep(0.5)
                return await js("(() => { const d = document.querySelector('.hangar-detail .hmod-add .rta-input');"
                                " const dd = d.closest('.hmod-add').querySelector('.rta-dd, .rta-dropdown, .rta-menu, [class*=dropdown], [class*=drop]');"
                                " return dd ? dd.innerText : 'NODROP'; })()")

            def suggested(opts):
                return [l for l in str(opts).splitlines() if l.strip()]

            # ---- 1: Aegis Eclipse — nur die Datei-Racks ----
            await open_detail("Aegis Eclipse")
            check("eclipse-module-field-visible",
                  (await js("document.querySelector('.hangar-detail .hmod-add') ? 'yes' : 'no'")) == "yes")
            opts = await typeahead_options("")
            real = {m["name"] for m in FOR_SHIP["aegis eclipse"]["modules"]}
            sug = suggested(opts)
            check("eclipse-suggest-exactly-file-racks",
                  bool(sug) and "NODROP" not in str(opts) and set(sug) == real,
                  f"suggested={sug} erwartet={sorted(real)}")
            check("eclipse-no-foreign-modules",
                  "Galaxy" not in str(opts) and "Retaliator" not in str(opts)
                  and "Living" not in str(opts), str(opts)[:200])
            await shot("01-eclipse-racks.png")

            # ---- 2: Aegis Retaliator — die 11 Datei-Module ----
            await open_detail("Aegis Retaliator")
            check("retaliator-module-field-visible",
                  (await js("document.querySelector('.hangar-detail .hmod-add') ? 'yes' : 'no'")) == "yes")
            opts = await typeahead_options("")
            real = {m["name"] for m in FOR_SHIP["aegis retaliator"]["modules"]}
            sug = suggested(opts)
            check("retaliator-suggest-subset-of-file",
                  bool(sug) and "NODROP" not in str(opts) and all(n in real for n in sug),
                  f"suggested={sug[:6]} nicht in Datei: {[n for n in sug if n not in real][:5]}")
            check("retaliator-no-foreign",
                  "Galaxy" not in str(opts) and "Eclipse" not in str(opts), str(opts)[:200])
            opts = await typeahead_options("bomb")
            sug = suggested(opts)
            check("retaliator-query-bomb-only-racks",
                  all("Bomb Rack" in n for n in sug) and len(sug) == 4,
                  f"suggested={sug}")
            await shot("02-retaliator-11.png")

            # ---- 3: RSI Galaxy — die 3 Datei-Module ----
            await open_detail("RSI Galaxy")
            check("galaxy-module-field-visible",
                  (await js("document.querySelector('.hangar-detail .hmod-add') ? 'yes' : 'no'")) == "yes")
            opts = await typeahead_options("")
            real = {m["name"] for m in FOR_SHIP["rsi galaxy"]["modules"]}
            sug = suggested(opts)
            check("galaxy-suggest-exactly-file-modules",
                  bool(sug) and "NODROP" not in str(opts) and set(sug) == real,
                  f"suggested={sug} erwartet={sorted(real)}")
            await shot("03-galaxy-3.png")

            # ---- 4: Aegis Arrow — GAR KEIN Modul-Feld (S4 Kernregel) ----
            await open_detail("Aegis Arrow")
            check("arrow-module-field-hidden",
                  (await js("document.querySelector('.hangar-detail .hmod-add') ? 'VISIBLE' : 'hidden'")) == "hidden")
            note = await js("document.querySelector('.hangar-detail .hd-mods') ? document.querySelector('.hangar-detail .hd-mods').innerText : 'NONE'")
            check("arrow-no-entry-note",
                  "Keine Modul-Zuweisung" in str(note), str(note)[:200])
            await shot("04-arrow-no-field.png")

            # ---- 5: Settings — Section „Modul-Zuweisungen" + Button ----
            await key("F12")
            await asyncio.sleep(1.5)
            sec = await js("(() => { const h=[...document.querySelectorAll('.cfg-sect .h')].find(x=>x.innerText.toUpperCase().includes('MODUL-ZUWEISUNGEN')); return h ? h.closest('.cfg-sect').innerText : 'NONE'; })()")
            check("settings-section-present", "MODUL-ZUWEISUNGEN" in str(sec).upper(), str(sec)[:150])
            check("settings-source-shown", "module-overrides.json" in str(sec), str(sec)[:200])
            check("settings-age-shown", "vor " in str(sec))
            check("settings-no-gameversion", "4.10.2" not in str(sec), str(sec)[:200])
            btn = await js("(() => { const s=[...document.querySelectorAll('.cfg-sect')].find(x=>x.innerText.toUpperCase().includes('MODUL-ZUWEISUNGEN'));"
                           " const b=[...s.querySelectorAll('button')].find(b=>b.innerText.toUpperCase().includes('AKTUALISIEREN')); return b ? b.innerText : 'NOBTN'; })()")
            # CSS-Upper: Button-Label großvergleichend prüfen (innerText
            # liefert den gerenderten, uppercase-Text).
            check("settings-button-label",
                  "MODUL-ZUWEISUNGEN AKTUALISIEREN" in str(btn).upper(), str(btn))
            await js("(() => { const s=[...document.querySelectorAll('.cfg-sect')].find(x=>x.innerText.toUpperCase().includes('MODUL-ZUWEISUNGEN'));"
                     " [...s.querySelectorAll('button')].find(b=>b.innerText.toUpperCase().includes('AKTUALISIEREN')).click(); })()")
            await asyncio.sleep(0.5)
            note = await js("(() => { const s=[...document.querySelectorAll('.cfg-sect')].find(x=>x.innerText.toUpperCase().includes('MODUL-ZUWEISUNGEN'));"
                            " const n=s.querySelector('.cfg-note'); return n ? n.innerText : 'NONE'; })()")
            check("settings-refresh-note", "aktualisiert" in str(note), str(note))
            await js("(() => { const s=[...document.querySelectorAll('.cfg-sect')].find(x=>x.innerText.toUpperCase().includes('MODUL-ZUWEISUNGEN')); s.scrollIntoView({block:'center'}); return 'OK'; })()")
            await asyncio.sleep(0.4)
            await shot("05-settings.png")
    finally:
        server.terminate()
        proc.terminate()

    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())
