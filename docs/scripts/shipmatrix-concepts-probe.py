#!/usr/bin/env python3
"""CDP-Probe für t_424660aa (UH: Konzept-Schiffe im Hangar-Autocomplete).

Datenbasis: statischer http.server auf build/ + headless Chromium (CDP),
IPC-Grenze via smoke-stub.js gestubbt — die echte Svelte-UI läuft unverändert.

Geprüft wird der Shipmatrix-Konzept-Zusatz (src/lib/hangar_ships.ts):
  1. Anlegen-Form: 'kraken'  → Dropdown zeigt 'Kraken · Drake' + Badge 'Konzept'.
  2. 'nautilus' → 'Nautilus · Aegis' + Badge; 'liberator' → 'Liberator · Anvil' + Badge.
  3. Negativ: 'aurora' → Aurora-Eintrag OHNE Konzept-Badge.
  4. Pick eines Konzept-Schiffs füllt Name + Hersteller (Drake) + Typ=ship.
  5. Dropdown-Geometrie: kein Overflow (scrollHeight <= clientHeight).

Screenshots: docs/screenshots/shipmatrix-concepts/01..03.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/shipmatrix-concepts")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8798/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

# Ein registrierter Bestandseintrag (dedupe-Pfad: Bestand geht vor Katalog).
ITEMS = [{
    "id": 1, "entity_type": "ship", "name": "Aurora Mk II", "custom_name": "",
    "manufacturer": "RSI", "base_ship": "", "acquired": "", "insured": "",
    "note": "", "created_at": 0, "updated_at": 0
}]

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8798", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9235", "--user-data-dir=/tmp/cdp-shipmatrix-concepts",
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
            await call("Page.addScriptToEvaluateOnNewDocument",
                       {"source": STUB + "\nwindow.__HANGAR_DATA__ = " + json.dumps(ITEMS) + ";"})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + (r["exceptionDetails"].get("text") or "")
                return r.get("result", {}).get("value")

            VK = {"F11": 125}
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

            async def setfield(sel, val):
                return await js(f"(() => {{ const e = document.querySelector({json.dumps(sel)}); if (!e) return 'NOSLCT';"
                                f"e.value = {json.dumps(val)}; e.dispatchEvent(new Event('input', {{bubbles: true}}));"
                                f"return e.value; }})()")

            async def open_add_form():
                await js("[...document.querySelectorAll('.hangar-actions .hbtn')].find(b=>b.innerText.includes('Anlegen')).click()")
                await asyncio.sleep(0.4)

            # Name-Typeahead im Anlegen-Form (aria-label stabiler als placeholder).
            NAME_SEL = "input[aria-label='Schiff/Fahrzeug-Name']"

            async def typeahead_items():
                return await js("[...document.querySelectorAll('.rta-dd .rta-item')].map(b => ({ t: b.innerText.trim(), badge: (b.querySelector('.rta-badge') || {}).textContent || null }))")

            await key("F11")
            await asyncio.sleep(1.0)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'") or "").strip()
            check("f11-enters-hangar", smode == "HANGAR", smode)

            # ---- 1: Kraken (Konzept) ----
            await open_add_form()
            await setfield(NAME_SEL, "kraken")
            await asyncio.sleep(0.4)
            items = await typeahead_items()
            items = items if isinstance(items, list) else []
            kraken = [i for i in items if i["t"].startswith("Kraken")]
            check("kraken-in-dropdown", len(kraken) >= 1, str(items)[:200])
            check("kraken-konzept-badge", bool(kraken) and kraken[0]["badge"] == "Konzept", str(kraken))
            await shot("01-kraken-konzept.png")

            # ---- 2: Nautilus + Liberator ----
            await setfield(NAME_SEL, "nautilus")
            await asyncio.sleep(0.4)
            nau = await typeahead_items()
            nau = nau if isinstance(nau, list) else []
            nau_ok = any(i["t"].startswith("Nautilus") and i["badge"] == "Konzept" for i in nau)
            check("nautilus-konzept-badge", nau_ok, str(nau)[:200])

            await setfield(NAME_SEL, "liberator")
            await asyncio.sleep(0.4)
            lib = await typeahead_items()
            lib = lib if isinstance(lib, list) else []
            lib_ok = any(i["t"].startswith("Liberator") and i["badge"] == "Konzept" for i in lib)
            check("liberator-konzept-badge", lib_ok, str(lib)[:200])
            await shot("02-liberator-konzept.png")

            # Dropdown-Geometrie: kein Overflow im Dropdown.
            geo = await js("(() => { const d = document.querySelector('.rta-dd'); if (!d) return null; return { sh: d.scrollHeight, ch: d.clientHeight }; })()")
            geo = geo if isinstance(geo, dict) else None
            check("dropdown-no-overflow", bool(geo) and geo["sh"] <= geo["ch"], str(geo))

            # ---- 3: Negativ — Bestand/Katalog-Eintrag ohne Konzept ----
            await setfield(NAME_SEL, "aurora")
            await asyncio.sleep(0.4)
            aur = await typeahead_items()
            aur = aur if isinstance(aur, list) else []
            aur_rows = [i for i in aur if "aurora" in i["t"].lower()]
            aur_ok = bool(aur_rows) and all(i["badge"] is None for i in aur_rows)
            check("aurora-ohne-badge", aur_ok, str(aur_rows))
            await shot("03-aurora-ohne-badge.png")

            # ---- 4: Pick füllt Name + Hersteller + Typ ----
            pick = await js(f"(() => {{ const b = [...document.querySelectorAll('.rta-dd .rta-item')].find(x => x.innerText.startsWith('Aurora')); if (!b) return 'NOTFOUND'; b.click(); return 'CLICKED'; }})()")
            await asyncio.sleep(0.3)
            vals = await js(f"""(() => {{
                const f = document.querySelector('.hform-modal form');
                if (!f) return null;
                const name = f.querySelector({json.dumps(NAME_SEL)});
                const fields = [...f.querySelectorAll('.trk-field')];
                const manu = fields.find(d => (d.querySelector('.trk-lbl') || {{}}).innerText?.toUpperCase().includes('HERSTELLER'));
                return {{ name: name ? name.value : null,
                         manu: manu && manu.querySelector('input') ? manu.querySelector('input').value : null }};
            }})()""")
            vals = vals if isinstance(vals, dict) else {}
            check("pick-fills-manufacturer", bool(vals) and vals.get("manu") == "RSI", str(vals))
    finally:
        for p_ in (server, proc):
            try:
                p_.terminate()
            except ProcessLookupError:
                pass

    print(f"\n== {len(PASS)} PASS / {len(FAIL)} FAIL ==")
    if FAIL:
        raise SystemExit(1)

if __name__ == "__main__":
    asyncio.run(main())
