#!/usr/bin/env python3
"""CDP-Probe für t_d0076295 (SA: Modul-Match Galaxy/Aurora MK II +
'Keine Zuweisung'-Hinweistext weg).

IPC-Grenze gestubbt (smoke-stub.js), echte Svelte-UI. Die Zuordnung kommt
aus dem Live-Test `live_sync_file_only_and_decision`
(docs/scripts/s4-modul-fixture.json — echte Repo-Baseline (Historie: Gitea-Raw) + reale
modul_for_ship_decision-Logik), nur umgeschlüsselt auf die ECHTEN
Hangar-Namen (Live-Dump 2026-10-30: „Galaxy“, „Aurora Mk II“ — OHNE
Hersteller-Präfix):

  1. Detail „Galaxy“ (Hangar-Name ohne RSI-Präfix) → Modul-Bereich mit den
     3 Datei-Modulen (Cargo/Med Bay/Refinery).
  2. Detail „Aurora Mk II“ → Modul-Bereich mit den 2 Datei-Modulen (DM/TS).
  3. Detail „Aurora Mk I SE“ (KEIN Datei-Eintrag) → GAR KEIN Text über
     Module (weder „Keine Modul-Zuweisung“ noch Feld noch Bereich).
  4. Regression: Eclipse + Retaliator matchen weiter (v5.9-Grün-Fälle).

Screenshots: 01-galaxy-module.png, 02-aurora-mk2-module.png,
03-no-entry-no-module.png, 04-eclipse-regression.png.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/sa-modul-name-match")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8796/"

FIXTURE = json.loads((ROOT / "docs/scripts/s4-modul-fixture.json").read_text())["for_ship"]
assert len(FIXTURE["rsi galaxy"]["modules"]) == 3
assert len(FIXTURE["rsi aurora mk ii"]["modules"]) == 2
assert FIXTURE["aegis arrow"]["known"] is False

FOR_SHIP = {
    "galaxy": FIXTURE["rsi galaxy"],
    "aurora mk ii": FIXTURE["rsi aurora mk ii"],
    "aegis eclipse": FIXTURE["aegis eclipse"],
    "aegis retaliator": FIXTURE["aegis retaliator"],
    "aurora mk i se": {"known": False, "has_ports": False, "modules": []},
}
MODUL_STATUS = {
    "version": "module-overrides.json", "synced_at": 1760000000,
    "age_seconds": 5400, "module_count": 36, "ship_count": 17,
}

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + str(detail)) if detail and not ok else ""))

def hangar_entry(id_, name, manufacturer):
    return {"id": id_, "name": name, "origin": "pledge", "insurance": "LTI",
            "custom_name": "", "entity_type": "ship", "base_ship": "", "ship_code": "",
            "manufacturer": manufacturer, "purchase_price": None,
            "purchase_date": "", "pledge_id": "", "pledge_name": "", "warbond": 0,
            "ccud": 0, "created_at": 1700000000, "updated_at": 1700000000}

HANGAR = [
    hangar_entry(1, "Galaxy", "Roberts Space Industries"),   # Live-Hangar-Name
    hangar_entry(2, "Aurora Mk II", "Roberts Space Industries"),
    hangar_entry(3, "Aurora Mk I SE", "Roberts Space Industries"),  # kein Eintrag
    hangar_entry(4, "Aegis Eclipse", "Aegis Dynamics"),      # Regression v5.9
    hangar_entry(5, "Aegis Retaliator", "Aegis Dynamics"),   # Regression v5.9
]

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8796", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9233", "--user-data-dir=/tmp/cdp-sa-modul",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9233/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception: pass
        await asyncio.sleep(0.25)

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
                    m = json.loads(raw)
                    if "id" in m and m["id"] in pending: pending[m["id"]].set_result(m)
            asyncio.create_task(recv_loop())
            def ok(resp):
                if "error" in resp: raise RuntimeError("CDP error: %s" % resp["error"])
                return resp.get("result", {})
            async def evaljs(expr):
                r = ok(await call("Runtime.evaluate",
                                  {"expression": expr, "returnByValue": True}))
                return r.get("result", {}).get("value")
            async def shot(name):
                r = ok(await call("Page.captureScreenshot", {"format": "png"}))
                (OUT / name).write_bytes(base64.b64decode(r["data"]))
                print(f"  shot {OUT / name}")

            await call("Page.enable")
            await call("Runtime.enable")
            await call("Emulation.setDeviceMetricsOverride",
                       {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
            await call("Page.addScriptToEvaluateOnNewDocument", {"source":
                STUB + "\nwindow.__HANGAR_DATA__ = " + json.dumps(HANGAR) + ";" +
                "\nwindow.__MODUL_FOR_SHIP__ = " + json.dumps(FOR_SHIP) + ";" +
                "\nwindow.__MODUL_STATUS__ = " + json.dumps(MODUL_STATUS) + ";"})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            # Hangar-View öffnen (F11-Hotkey, wie in den anderen Probes).
            await call("Input.dispatchKeyEvent", {"type": "keyDown", "windowsVirtualKeyCode": 122, "key": "F11"})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "windowsVirtualKeyCode": 122, "key": "F11"})
            await asyncio.sleep(1.2)

            body = "document.body.innerText"
            for name, shot_name, expect_field in [
                ("Galaxy", "01-galaxy-module.png", True),
                ("Aurora Mk II", "02-aurora-mk2-module.png", True),
                ("Aurora Mk I SE", "03-no-entry-no-module.png", False),
                ("Aegis Eclipse", "04-eclipse-regression.png", True),
                ("Aegis Retaliator", "05-retaliator-regression.png", True),
            ]:
                opened = await evaljs(f"""(() => {{
                    const rows = [...document.querySelectorAll('.hrow')];
                    const row = rows.find(r => r.textContent.includes('{name}'));
                    if (!row) return 'no-row';
                    row.click(); return 'ok';
                }})()""")
                if opened != "ok":
                    check(f"detail {name}: Zeile gefunden", False, opened); continue
                await asyncio.sleep(1.0)
                txt = await evaljs(body) or ""
                mods = await evaljs(
                    "document.querySelectorAll('.hd-mods').length + '/' + "
                    "document.querySelectorAll('.hmod-add').length")
                if expect_field:
                    check(f"{name}: Modul-Bereich mit Hinzufügen-Feld", mods == "1/1", mods)
                    check(f"{name}: kein 'Keine Modul-Zuweisung'-Text",
                          "Keine Modul-Zuweisung" not in txt)
                    # Datei-Module im Autocomplete belegen (Galaxy: Refinery).
                    if name == "Galaxy":
                        await evaljs("""(() => {
                            const inp = document.querySelector('.hmod-add input, .hmod-add .rta-input input, .hmod-add [contenteditable]');
                            if (!inp) return 'no-input';
                            inp.value = 'Refinery';
                            inp.dispatchEvent(new Event('input', {bubbles: true}));
                            return 'ok';
                        })()""")
                        await asyncio.sleep(0.6)
                        txt2 = await evaljs(body) or ""
                        check("Galaxy: Refinery-Modul im Autocomplete", "Refinery" in txt2, txt2[-300:])
                else:
                    check(f"{name}: GAR KEIN Modul-Bereich/-Text",
                          mods == "0/0" and "Module" not in txt and
                          "Keine Modul-Zuweisung" not in txt, f"{mods} | {txt[-300:]}")
                await shot(shot_name)
                # zurück zur Liste (Esc — genau einmal, bis Rows wieder da).
                for _ in range(4):
                    rows_n = await evaljs("document.querySelectorAll('.hrow').length")
                    if rows_n:
                        break
                    await call("Input.dispatchKeyEvent", {"type": "keyDown", "windowsVirtualKeyCode": 27, "key": "Escape"})
                    await call("Input.dispatchKeyEvent", {"type": "keyUp", "windowsVirtualKeyCode": 27, "key": "Escape"})
                    await asyncio.sleep(0.6)

            print(f"\n  {len(PASS)} PASS / {len(FAIL)} FAIL")
            if FAIL:
                raise SystemExit(1)
    finally:
        server.terminate(); proc.terminate()

asyncio.run(main())
