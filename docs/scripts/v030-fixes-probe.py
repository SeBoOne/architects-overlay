#!/usr/bin/env python3
"""CDP-Probe für t_a39f6811 (v0.3.0-Fixes: F6-Routen, Gear raus, CTRL+M raus).

Datenquelle: smoke-stub.js (IPC-Grenze gestubbt, echte Svelte-UI). Checks:
  1. F6 → Marktmodus mit Handelsrouten-Tab (mTab=routes), Rail-Eintrag F6 aktiv.
  2. F2 → zurück auf Angebote-Tab.
  3. Zahnrad-Button (.gear) in der Statuszeile fehlt; Statuszeile (FOKUS/KLICK,
     Sync-Status) bleibt.
  4. CTRL+M: im Market-Modus mit Fokus im Suchfeld löst KEIN Verlassen aus;
     'm' tippt normal ins Suchfeld.
  5. F4 → Hangar-Add: hangar_add läuft ohne 'state not managed'-Pfad
     (Stub-Spiegel + Note 'Angelegt').
Screenshots: 01-f6-routes.png, 02-f2-offers.png, 03-no-gear.png, 04-f4-hangar-add.png.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/v030-fixes")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8797", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-v030fixes",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9234/json/list", timeout=1) as r:
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
                       {"source": STUB + "\nwindow.__HANGAR_DATA__ = [{ id: 1, name: 'Cutter', origin: 'pledge', entity_type: 'ship', modules: [] }];"})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + (r["exceptionDetails"].get("text") or "")
                return r.get("result", {}).get("value")

            VK = {"F2": 113, "F4": 115, "F6": 117, "Escape": 27, "m": 77, "Control": 17}
            async def key(k, mods=()):
                vk = VK[k]
                for m in mods:
                    await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": m, "code": m,
                                                          "windowsVirtualKeyCode": VK[m], "nativeVirtualKeyCode": VK[m]})
                await call("Input.dispatchKeyEvent", {"type": "keyDown", "key": k, "code": k,
                                                      "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk, "text": k if len(k) == 1 else ""})
                await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                      "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
                for m in mods:
                    await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": m, "code": m,
                                                          "windowsVirtualKeyCode": VK[m], "nativeVirtualKeyCode": VK[m]})
                await asyncio.sleep(0.35)

            async def shot(name):
                r = ok(await call("Page.captureScreenshot", {"format": "png"}))
                (OUT / name).write_bytes(base64.b64decode(r["data"]))
                print("  shot:", OUT / name)

            # ---- 1. F6 → Handelsrouten-Tab ----
            await key("F6")
            await asyncio.sleep(1.2)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()
            check("f6-smode-handlesrouten", smode == "HANDELSROUTEN", smode)
            tab_on = await js("document.querySelector('.chip.is-on') ? document.querySelector('.chip.is-on').innerText.trim() : 'NONE'")
            check("f6-routes-tab-active", tab_on.lower() == "handelsrouten", tab_on)
            rail_f6 = await js("[...document.querySelectorAll('.rentry')].find(b=>b.querySelector('.rkey')?.innerText==='F6') ? document.querySelector('.rentry.is-on .rkey')?.innerText : 'NONE'")
            check("f6-rail-entry-active", rail_f6 == "F6", str(rail_f6))
            await shot("01-f6-routes.png")

            # ---- 2. F2 → Angebote ----
            await key("F2")
            await asyncio.sleep(0.6)
            tab_on = await js("document.querySelector('.chip.is-on') ? document.querySelector('.chip.is-on').innerText.trim() : 'NONE'")
            check("f2-back-to-offers", tab_on.lower() == "angebote", tab_on)

            # ---- 3. Zahnrad weg, Statuszeile bleibt ----
            gear = await js("document.querySelector('.gear') ? 'present' : 'absent'")
            check("gear-button-removed", gear == "absent", gear)
            sync = await js("document.querySelector('.sync') ? 'present' : 'absent'")
            check("sync-status-still-there", sync == "present", sync)
            spot = await js("document.querySelector('.spot-status') ? 'present' : 'absent'")
            check("spot-status-still-there", spot == "present", spot)
            await shot("03-no-gear.png")

            # ---- 4. CTRL+M ohne Effekt, 'm' tippt normal ----
            await key("F2")  # sicher im Market-Modus
            await asyncio.sleep(0.5)
            await js("document.querySelector('input.sinput, input')?.focus()")
            await asyncio.sleep(0.2)
            before = await js("document.querySelector('.chip.is-on') ? document.querySelector('.chip.is-on').innerText.trim() : 'NONE'")
            await key("m", mods=("Control",))
            after = await js("document.querySelector('.chip.is-on') ? document.querySelector('.chip.is-on').innerText.trim() : 'NONE'")
            check("ctrl-m-no-exit", before == after, f"{before} -> {after}")
            # 'm' tippt in das fokussierte Suchfeld
            await js("var i=document.querySelector('input.sinput, input'); i.value=''; i.focus(); i.dispatchEvent(new Event('input',{bubbles:true}));")
            await key("m")
            val = await js("document.querySelector('input.sinput, input') ? document.querySelector('input.sinput, input').value : 'NONE'")
            check("m-types-into-field", val == "m", val)
            await key("Escape")

            # ---- 5. F4 → Hangar-Anlegen ohne Fehler ----
            await key("F4")
            await asyncio.sleep(0.8)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()
            check("f4-enters-hangar", smode == "HANGAR", smode)
            await js("[...document.querySelectorAll('.hangar-actions .hbtn')].find(b=>b.innerText.includes('Anlegen'))?.click()")
            await asyncio.sleep(0.3)
            await js("var n=document.querySelector('form.hangar-form input[placeholder*=Cutlass]'); if(n){n.value='Probe Cutter';n.dispatchEvent(new Event('input',{bubbles:true}));}")
            await js("document.querySelector('form.hangar-form button[type=submit]').click()")
            await asyncio.sleep(0.6)
            note = (await js("document.querySelector('.hangar-note') ? document.querySelector('.hangar-note').innerText : ''")).strip()
            check("hangar-add-ok", "Angelegt" in note, note)
            last = await js("String(window.__HANGAR_DATA__.length) + ':' + (window.__HANGAR_DATA__.find(i=>i.name==='Probe Cutter') ? 'found' : 'missing')")
            check("hangar-add-invoked", last.startswith("2:") and last.endswith("found"), str(last))
            await shot("04-f4-hangar-add.png")
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
