#!/usr/bin/env python3
"""CDP-Probe für t_317f19c5 (Hangar-Modus: Übersicht/Detail/Anlegen).

Datenquelle: ECHTER Import — `ARCH_HANGAR_DUMP=/tmp/hangar-smoke.json cargo
test -p architects-overlay --lib dump_real_import_for_smoke` fährt den echten
Parser+Merge (hangar.rs) gegen /tmp/HangarExport (XPLorer-JSON + CCUgame-CSV)
und schreibt das hangar_list-Ergebnis. Diese Probe stubbt nur die IPC-Grenze
(smoke-stub.js) und verifiziert die echte Svelte-UI:
  1. F4 → Hangar-Übersicht: 28 Einträge (Golem Duo Pack bleibt 2 Zeilen),
     Chips PLEDGE + Versicherung sichtbar.
  2. Schiffs-Detail: Werte + Module-Block, Bearbeiten-Formular.
  3. Anlegen-Formular: Felder da, Speichern ruft hangar_add.
Screenshots: 01-hangar-list.png, 02-hangar-detail.png, 03-hangar-add-form.png.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
DATA = pathlib.Path("/tmp/hangar-smoke.json")
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/hangar")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8796/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

async def main():
    if not DATA.exists():
        raise RuntimeError(
            "/tmp/hangar-smoke.json fehlt — erst: ARCH_HANGAR_DUMP=/tmp/hangar-smoke.json "
            "cargo test -p architects-overlay --lib dump_real_import_for_smoke")
    items = json.loads(DATA.read_text())

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
        "--remote-debugging-port=9233", "--user-data-dir=/tmp/cdp-hangar",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9233/json/list", timeout=1) as r:
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
                       {"source": STUB + "\nwindow.__HANGAR_DATA__ = " + json.dumps(items) + ";"})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + (r["exceptionDetails"].get("text") or "")
                return r.get("result", {}).get("value")

            VK = {"F4": 115, "Escape": 27, "Enter": 13, "ArrowDown": 40}
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

            # ---- F4 → Hangar-Übersicht ----
            await key("F4")
            await asyncio.sleep(1.0)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()
            check("f4-enters-hangar", smode == "HANGAR", smode)
            rows = int(await js("document.querySelectorAll('.hangar-list .hrow').length"))
            check("list-28-rows", rows == 28, "rows=" + str(rows))
            golems = int(await js("document.querySelectorAll('.hangar-list .hrow').length && "
                                  "[...document.querySelectorAll('.hname')].filter(e=>e.textContent.includes('Golem')).length"))
            check("golem-duo-pack-2-rows", golems == 2, "golem-rows=" + str(golems))
            chips = int(await js("document.querySelectorAll('.hchip.o-pledge').length"))
            check("pledge-chips-present", chips == 28, "pledge-chips=" + str(chips))
            ins = int(await js("[...document.querySelectorAll('.hchip.ins')].filter(e=>e.textContent.trim()==='LTI').length"))
            check("lti-insurance-visible", ins >= 20, "lti-chips=" + str(ins))
            await shot("01-hangar-list.png")

            # ---- Detail (Tastatur: ↓ + Enter analog Inventar) ----
            await key("ArrowDown")
            await key("Enter")
            det = await js("document.querySelector('.hangar-detail') ? 'yes' : 'no'")
            check("detail-open", det == "yes", det)
            name = (await js("document.querySelector('.hd-name') ? document.querySelector('.hd-name').innerText : 'NONE'")).strip()
            check("detail-name", name and name != "NONE", name)
            await shot("02-hangar-detail.png")

            # ---- Bearbeiten-Formular im Detail ----
            edit_btns = await js("[...document.querySelectorAll('.hangar-detail .hbtn')].map(b=>b.innerText)")
            has_edit = any("Bearbeiten" in b for b in (edit_btns or []))
            check("detail-edit-button", bool(has_edit), str(edit_btns))
            if has_edit:
                await js("[...document.querySelectorAll('.hangar-detail .hbtn')].find(b=>b.innerText.includes('Bearbeiten')).click()")
                await asyncio.sleep(0.3)
                form = await js("document.querySelector('.hangar-detail form.hangar-form') ? 'yes' : 'no'")
                check("edit-form-visible", form == "yes", form)
                await js("document.querySelector('.hangar-detail form.hangar-form input[placeholder*=LTI]').value='LTI+90';"
                         "document.querySelector('.hangar-detail form.hangar-form input[placeholder*=LTI]').dispatchEvent(new Event('input',{bubbles:true}));")
                await js("document.querySelector('.hangar-detail form.hangar-form button[type=submit]').click()")
                await asyncio.sleep(0.5)
                note = (await js("document.querySelector('.hangar-note') ? document.querySelector('.hangar-note').innerText : ''")).strip()
                check("edit-saved", "Aktualisiert" in note, note)
                await key("Escape")  # zurück zur Liste

            # ---- Anlegen-Formular ----
            await js("[...document.querySelectorAll('.hangar-actions .hbtn')].find(b=>b.innerText.includes('Anlegen')).click()")
            await asyncio.sleep(0.3)
            form = await js("document.querySelector('form.hangar-form .hf-title') ? document.querySelector('form.hangar-form .hf-title').innerText : 'NONE'")
            check("add-form-visible", form == "EINTRAG ANLEGEN", form)
            fields = int(await js("document.querySelectorAll('form.hangar-form .hf-grid label').length"))
            check("add-form-fields", fields >= 9, "fields=" + str(fields))
            # Ingame-Kauf anlegen: origin-Select auf ingame, Typ ship
            await js("var n=document.querySelector('form.hangar-form input[placeholder*=Cutlass]');"
                     "n.value='My Ingame Cutter';n.dispatchEvent(new Event('input',{bubbles:true}));")
            await shot("03-hangar-add-form.png")
            before = int(await js("window.__HANGAR_DATA__.length"))
            await js("document.querySelector('form.hangar-form button[type=submit]').click()")
            await asyncio.sleep(0.5)
            after = int(await js("window.__HANGAR_DATA__.length"))
            check("add-calls-backend", after == before + 1, f"{before}->{after}")
            note = (await js("document.querySelector('.hangar-note') ? document.querySelector('.hangar-note').innerText : ''")).strip()
            check("add-note", "Angelegt" in note, note)
            await shot("04-hangar-added.png")
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
