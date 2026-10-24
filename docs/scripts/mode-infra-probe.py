#!/usr/bin/env python3
"""CDP-Probe für t_315a26cc (Modi-Infrastruktur: F-Tasten, Rail, F12-Settings, ESC-Rework).

Belegt auf der echten Svelte-Oberfläche (echte Logik, Tauri-IPC gestubbt):
  1. Rail: 5 Einträge (F1/F2/F3/F4/F12), aktiver Modus orange markiert.
  2. F-Tasten als In-App-Keyhandler: F2 → Market, F12 → Settings,
     F3/F4 → Platzhalter, F1 → zurück zur Suche; Fokus bleibt im Suchfeld.
  3. ESC-Rework: ESC ruft NIE overlay_set_visible(false) auf — Detail → Liste,
     Market → Suche, Platzhalter → Suche; im Root tut ESC nichts (Input bleibt).
  4. F-Tasten geraten nicht in Suchfeld-Eingaben (Text bleibt unangetastet).
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/mode-infra")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8795/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8795", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9229", "--user-data-dir=/tmp/cdp-mode-infra",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9229/json/list", timeout=1) as r:
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

        VK = {"F1": 112, "F2": 113, "F3": 114, "F4": 115, "F12": 123, "Escape": 27, "Enter": 13}
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

        # overlay_set_visible-Aufrufe aufzeichnen (ESC-Rework-Nachweis)
        await js("window.__calls = [];"
                 "window.__TAURI_INTERNALS__.origInvoke = window.__TAURI_INTERNALS__.invoke;"
                 "window.__TAURI_INTERNALS__.invoke = function(cmd,args){"
                 "window.__calls.push(cmd);"
                 "return window.__TAURI_INTERNALS__.origInvoke(cmd,args);};")

        # ---- Rail: Struktur ----
        n = int(await js("document.querySelectorAll('.rentry').length"))
        check("rail-present", n == 5, "entries=" + str(n))
        keys = json.loads(await js("JSON.stringify([...document.querySelectorAll('.rentry .rkey')].map(function(e){return e.textContent.trim();}))"))
        check("rail-keys", keys == ["F1", "F2", "F3", "F4", "F12"], str(keys))
        active = await js("document.querySelector('.rentry.is-on .rkey') ? document.querySelector('.rentry.is-on .rkey').textContent.trim() : 'NONE'")
        check("f1-active-on-load", active == "F1", active)

        # ---- Suche + F2 → Market (In-App-Keyhandler, Fokus bleibt im Suchfeld) ----
        await js("var i=document.querySelector('input.q'); if(i) i.focus();")
        await key("F2")
        smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()
        check("f2-enters-market", smode == "MARKETPLACE", smode)
        active = await js("document.querySelector('.rentry.is-on .rkey') ? document.querySelector('.rentry.is-on .rkey').textContent.trim() : 'NONE'")
        check("rail-market-active", active == "F2", active)
        ae = await js("document.activeElement === document.querySelector('input.q')")
        check("focus-search-after-f2", bool(ae))
        await shot("02-rail-market.png")

        # ---- ESC im Market: zurück zur Suche, KEIN Overlay-hide ----
        await js("window.__calls.length = 0;")
        await key("Escape")
        smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()
        check("esc-market-back-to-search", smode == "SUCHE", smode)
        hide_calls = int(await js("window.__calls.filter(function(c){return c==='overlay_set_visible';}).length"))
        check("esc-market-no-overlay-hide", hide_calls == 0, "overlay_set_visible calls=" + str(hide_calls))

        # ---- F12 → Settings, F1 zurück ----
        await key("F12")
        cfg = await js("!!document.querySelector('.pane-config')")
        check("f12-opens-settings", bool(cfg))
        active = await js("document.querySelector('.rentry.is-on .rkey') ? document.querySelector('.rentry.is-on .rkey').textContent.trim() : 'NONE'")
        check("rail-settings-active", active == "F12", active)
        await shot("03-rail-settings.png")
        await key("F1")
        smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()
        check("f1-back-to-search", smode == "SUCHE", smode)

        # ---- F3/F4 → Platzhalter, Esc zurück ----
        await key("F3")
        # t_1bab1fea: F3 zeigt jetzt den ECHTEN Inventar-Modus (kein Platzhalter).
        inv = await js("!!document.querySelector('.pane-inventory')")
        check("f3-opens-inventory", bool(inv), "pane-inventory missing")
        smode3 = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()
        check("f3-smode-inventar", smode3 == "INVENTAR", smode3)
        await key("Escape")
        inv_gone = await js("!document.querySelector('.pane-inventory')")
        check("esc-inventory-back", bool(inv_gone))
        await key("F4")
        ph = (await js("document.querySelector('.pane-placeholder .big') ? document.querySelector('.pane-placeholder .big').innerText : 'NONE'")).strip()
        check("f4-hangar-placeholder", ph == "HANGAR", ph)
        await key("F1")

        # ---- Suche: Trefferliste → Detail → ESC (interne Navigation, kein Hide) ----
        def set_input_expr(sel, text):
            return ("(function(){var i=document.querySelector(" + json.dumps(sel) + ");"
                    "if(!i) return 'NO-ELEMENT';"
                    "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                    "set.call(i,'');"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                    "set.call(i," + json.dumps(text) + ");"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                    "return i.value;})()")
        await js(set_input_expr("input.q", "gold"))
        await asyncio.sleep(1.0)
        rows = int(await js("document.querySelectorAll('.row').length"))
        check("search-gold-rows", rows >= 3, "rows=" + str(rows))
        await shot("01-rail-search.png")
        # Detail öffnen (Enter auf Selektion 0)
        await key("Enter")
        await asyncio.sleep(0.8)
        detail = await js("!!document.querySelector('.pane-detail')")
        check("detail-open", bool(detail))
        await js("window.__calls.length = 0;")
        await key("Escape")
        await asyncio.sleep(0.4)
        detail_gone = await js("!document.querySelector('.pane-detail')")
        check("esc-detail-back-to-list", bool(detail_gone))
        hide_calls = int(await js("window.__calls.filter(function(c){return c==='overlay_set_visible';}).length"))
        check("esc-detail-no-overlay-hide", hide_calls == 0, "overlay_set_visible calls=" + str(hide_calls))

        # ---- ESC im Root: tut NICHTS (kein Hide, kein Leeren) ----
        keep = await js("document.querySelector('input.q').value")
        await js("window.__calls.length = 0;")
        await key("Escape")
        await asyncio.sleep(0.3)
        hide_calls = int(await js("window.__calls.filter(function(c){return c==='overlay_set_visible';}).length"))
        check("esc-root-no-overlay-hide", hide_calls == 0, "overlay_set_visible calls=" + str(hide_calls))
        still = await js("document.querySelector('input.q').value")
        check("esc-root-keeps-input", still == keep, "before=" + repr(keep) + " after=" + repr(still))

        # ---- F-Tasten geraten nicht in Suchfeld-Eingaben ----
        await js(set_input_expr("input.q", "l"))
        await js("var i=document.querySelector('input.q'); i.focus(); i.setSelectionRange(i.value.length, i.value.length);")
        await key("F2")  # wechselt in Market UND darf nichts ins Feld schreiben
        val = await js("document.querySelector('input.q').value")
        check("fkey-not-typed-into-search", val == "l", repr(val))
        await key("F1")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS, {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())
