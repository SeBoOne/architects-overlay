#!/usr/bin/env python3
"""CDP-Probe Q2 (t_56d5264a) — Detail-Enter nur Commodities + Overlay-Höhe.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  QA — Commodity-Detail: Enter auf einer Preiszeile öffnet das Trade-Erfassen-
       Popup VORAUSGEFÜLLT (Richtung/Ware/Preis/Ort aus der Zeile), keine
       is-chosen-Markierung mehr, kein „Auswahl“-Toast.
  QB — Popup: Escape bricht ab (Popup zu, Detail bleibt); Enter im SCU-Feld
       bestätigt (2-Schritt-Bestätigung „Trade ANLEGEN?“ erscheint).
  QC — Item-Detail: Enter macht NICHTS (kein Popup, keine Auswahl).
  QD — Terminal-Detail: Enter macht NICHTS.
  QE — Listing-Popup offen → Shell temporär höher (max. nutzbare Höhe),
       nach dem Schließen wieder die alte Höhe. Kein Hyprland-Dispatch —
       das Overlay-Fenster spannt ohnehin den ganzen Monitor (overlay.rs,
       place_on_pointer_monitor); die sichtbare Overlay-Höhe ist die Shell-Box.

Datenquelle: smoke-stub.js (JS-Mirror; echte IPC-Grenze durch cargo tests).
Screenshots nach docs/screens/q2-detail-enter/.
"""
import asyncio, base64, json, os, pathlib, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens/q2-detail-enter"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8812"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9252"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
        "--directory", str(ROOT / "build"),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    import urllib.request
    for _ in range(60):
        try:
            with urllib.request.urlopen(URL, timeout=1) as r:
                if r.status == 200:
                    break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    else:
        raise RuntimeError(f"static server on {PORT} did not come up")
    print("static server up on", PORT)

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-q2-enter",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{CDP_PORT}/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        raise RuntimeError("chromium CDP page target did not come up")

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
                m = json.loads(raw)
                if "id" in m and m["id"] in pending:
                    pending[m["id"]].set_result(m)
        asyncio.create_task(recv_loop())

        def ok(resp):
            if "error" in resp:
                raise RuntimeError("CDP error: %s" % resp["error"])
            return resp.get("result", {})

        await call("Page.enable")
        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1500, "deviceScaleFactor": 2, "mobile": False})

        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source":
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "window.__SHOP_TRADES__ = [];"
            "window.__SHOP_TRADES_SOURCE__ = 'cache';"})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.2)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        async def real_key(key, code, vk, down=True):
            ev = {"type": "keyDown" if down else "keyUp", "key": key, "code": code,
                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk}
            if down and key == "Enter":
                ev["text"] = "\r"
                ev["unmodifiedText"] = "\r"
            await call("Input.dispatchKeyEvent", ev)

        VK = {"Enter": (13, "Enter"), "ArrowDown": (40, "ArrowDown"), "Escape": (27, "Escape")}
        async def key(k):
            vk, code = VK[k]
            await real_key(k, code, vk, True)
            await real_key(k, code, vk, False)
            await asyncio.sleep(0.25)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = base64.b64decode(r["data"])
            (OUT / name).write_bytes(data)
            print("  shot:", OUT / name, len(data), "bytes")

        def type_input_js(selector, text):
            return ("(function(){"
                    "var i=document.querySelector(" + json.dumps(selector) + ");"
                    "if(!i) return 'NOFIELD';"
                    "i.focus();"
                    "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                    "set.call(i," + json.dumps(text) + ");"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'x'}));"
                    "return i.value;})()")

        async def search_and_open(q, pick="first"):
            await js("(function(){var i=document.querySelector('input'); if(i) i.focus();})()")
            await js(type_input_js("input", q))
            await asyncio.sleep(1.6)
            if pick == "first":
                await js("document.querySelector('.row')?.click()")
            else:
                await js("(function(){var r=[...document.querySelectorAll('.row')].find(function(x){return (x.textContent||'').includes(" + json.dumps(pick) + ")}); if(r) r.click();})()")
            await asyncio.sleep(1.4)

        # =================================================================
        # QA — Commodity-Detail: Enter → vorausgefülltes Trade-Erfassen-Popup
        # =================================================================
        await search_and_open("gold")
        rows = await js("document.querySelectorAll('.pane-detail .pr').length")
        check("qa-detail-open", (rows or 0) > 0, f"rows={rows}")
        sel0 = await js("document.querySelectorAll('.pane-detail .pr.is-sel').length")
        cho0 = await js("document.querySelectorAll('.pane-detail .pr.is-chosen').length")
        await key("ArrowDown")
        sel1 = await js("document.querySelectorAll('.pane-detail .pr.is-sel').length")
        cho1 = await js("document.querySelectorAll('.pane-detail .pr.is-chosen').length")
        check("qa-arrows-sel-no-chosen", sel1 == 1 and cho1 == 0, f"is-sel={sel1} is-chosen={cho1} (init sel={sel0} cho={cho0})")
        # erste sichtbare Zeile: Kauf-Seite → cheapest buy row price
        rowprice = await js("(function(){var r=document.querySelectorAll('.pane-detail .pr .val.buy')[0]; return r? r.textContent.trim():null;})()")
        await key("Enter")
        await asyncio.sleep(0.5)
        modal = await js("!!document.querySelector('.trk-modal')")
        check("qa-enter-opens-trk-modal", modal is True)
        if modal:
            dirn = await js("(function(){var on=document.querySelector('.trk-modal .trk-dir .chip.is-on'); return on?on.textContent.trim():null;})()")
            ware = await js("(function(){var f=[...document.querySelectorAll('.trk-modal .trk-field')]; var inp=f.length?f[1].querySelector('input'):null; return inp?inp.value:null;})()")
            ort = await js("(function(){var f=[...document.querySelectorAll('.trk-modal .trk-field')]; var inp=f.length>2?f[2].querySelector('input'):null; return inp?inp.value:null;})()")
            preis = await js("(function(){var f=[...document.querySelectorAll('.trk-modal .trk-field')]; var inp=f.length>4?f[4].querySelector('input'):null; return inp?inp.value:null;})()")
            menge = await js("(function(){var f=[...document.querySelectorAll('.trk-modal .trk-field')]; var inp=f.length>3?f[3].querySelector('input'):null; return inp?inp.value:'';})()")
            check("qa-prefill-direction-buy", dirn == "Kauf", str(dirn))
            check("qa-prefill-commodity", (ware or "").startswith("Gold"), str(ware))
            check("qa-prefill-location", bool(ort), str(ort))
            check("qa-prefill-price", bool(preis) and float(preis or 0) > 0, f"modal={preis} row={rowprice}")
            check("qa-menge-empty-for-user", menge == "", str(menge))
            # keine Auswahl-Markierung mehr, kein Toast
            cho2 = await js("document.querySelectorAll('.pane-detail .pr.is-chosen').length")
            toast = await js("!!document.querySelector('.d-toast')")
            check("qa-no-chosen-no-toast", cho2 == 0 and toast is False, f"chosen={cho2} toast={toast}")
            await shot("q2-qa-commodity-enter-popup.png")

            # QB — Escape bricht ab (Popup zu, Detail bleibt)
            await key("Escape")
            await asyncio.sleep(0.4)
            gone = await js("!document.querySelector('.trk-modal')")
            detail = await js("!!document.querySelector('.pane-detail')")
            check("qb-escape-closes-popup-keeps-detail", gone is True and detail is True, f"popup-gone={gone} detail={detail}")

            # QB2 — Enter im SCU-Feld bestätigt (2-Schritt)
            await key("Enter")
            await asyncio.sleep(0.4)
            again = await js("!!document.querySelector('.trk-modal')")
            check("qb2-enter-reopens", again is True)
            if again:
                await js(type_input_js(".trk-modal input[type=number]", "5"))
                await asyncio.sleep(0.3)
                await js("(function(){var i=document.querySelector('.trk-modal input[type=number]'); if(i) i.focus();})()")
                await key("Enter")
                await asyncio.sleep(0.6)
                confirm = await js("(function(){var m=document.querySelector('.confirm-modal'); return m? m.textContent.includes('Trade ANLEGEN?'): false;})()")
                check("qb2-enter-submits-2step", confirm is True)
                await shot("q2-qb-enter-2step.png")
                await key("Escape")  # Bestätigung schließen
                await asyncio.sleep(0.3)
                await key("Escape")  # Formular schließen
                await asyncio.sleep(0.3)

        # =================================================================
        # QC — Item-Detail: Enter macht NICHTS
        # =================================================================
        await js("(function(){location.reload();})()")
        await asyncio.sleep(2.2)
        await search_and_open("backpack")
        rows_item = await js("document.querySelectorAll('.pane-detail .pr').length")
        check("qc-item-detail-open", (rows_item or 0) > 0, f"rows={rows_item}")
        await key("ArrowDown")
        await key("Enter")
        await asyncio.sleep(0.5)
        no_modal = await js("!document.querySelector('.trk-modal')")
        no_chosen = await js("document.querySelectorAll('.pane-detail .pr.is-chosen').length")
        check("qc-item-enter-inert", no_modal is True and no_chosen == 0, f"modal={not no_modal} chosen={no_chosen}")
        await shot("q2-qc-item-enter-inert.png")

        # =================================================================
        # QD — Terminal-Detail: Enter macht NICHTS
        # =================================================================
        await js("(function(){location.reload();})()")
        await asyncio.sleep(2.2)
        await search_and_open("landing services", pick="Landing Services - Checkmate")
        rows_term = await js("document.querySelectorAll('.pane-detail .pr').length")
        check("qd-terminal-detail-open", (rows_term or 0) > 0, f"rows={rows_term}")
        await key("ArrowDown")
        await key("Enter")
        await asyncio.sleep(0.5)
        no_modal_t = await js("!document.querySelector('.trk-modal')")
        no_chosen_t = await js("document.querySelectorAll('.pane-detail .pr.is-chosen').length")
        check("qd-terminal-enter-inert", no_modal_t is True and no_chosen_t == 0, f"modal={not no_modal_t} chosen={no_chosen_t}")

        # =================================================================
        # QE — Listing-Popup: Shell temporär höher, Restore nach Schließen
        # =================================================================
        async def shell_h():
            return await js("(function(){var s=document.querySelector('.shell'); return s? Math.round(s.getBoundingClientRect().height):null;})()")
        async def shell_maxh():
            return await js("(function(){var s=document.querySelector('.shell'); return s? getComputedStyle(s).maxHeight:null;})()")
        def px(v):
            try:
                return float(str(v).replace("px", ""))
            except Exception:
                return 0.0
        await js("(function(){location.reload();})()")
        await asyncio.sleep(2.2)
        # Market-Modus (F2) → Shop-Tab → „Anlegen“-Chip klicken
        await js("(function(){var i=document.querySelector('input'); if(i) i.focus();})()")
        await js(type_input_js("input", "WARM"))
        await asyncio.sleep(0.8)
        await js(type_input_js("input", ""))
        await asyncio.sleep(0.4)
        # F2 via echter Taste
        await real_key("F2", "F2", 113, True)
        await real_key("F2", "F2", 113, False)
        await asyncio.sleep(1.4)
        # Mein Shop-Tab öffnen (dort sitzt der „Anlegen“-Chip)
        await js("(function(){var c=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()==='Mein Shop';}); if(c) c.click();})()")
        await asyncio.sleep(1.6)
        h_before = await shell_h()
        mh_before = await shell_maxh()
        opened = await js("(function(){var c=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()==='Anlegen';}); if(c){c.click(); return true;} return false;})()")
        await asyncio.sleep(0.8)
        adv = await js("!!document.querySelector('.advert-modal')")
        check("qe-advert-opens", opened is True and adv is True)
        h_open = await shell_h()
        mh_open = await shell_maxh()
        check("qe-shell-taller-with-popup", px(mh_open) > px(mh_before) + 40,
              f"maxHeight before={mh_before} open={mh_open} (rect {h_before}->{h_open})")
        await shot("q2-qe-shell-tall-popup.png")
        # schließen (Esc) → Restore
        for d in (True, False):
            await real_key("Escape", "Escape", 27, d)
        await asyncio.sleep(0.6)
        adv2 = await js("!document.querySelector('.advert-modal')")
        h_after = await shell_h()
        mh_after = await shell_maxh()
        check("qe-shell-restored", adv2 is True and abs(px(mh_after) - px(mh_before)) < 1,
              f"maxHeight before={mh_before} after={mh_after} (rect {h_before}->{h_after})")
        await shot("q2-qe-shell-restored.png")

    server.terminate()
    proc.terminate()

    print("")
    print(f"RESULT: {'PASS' if not FAIL else 'FAIL'} — {len(PASS)} passed, {len(FAIL)} failed")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
    raise SystemExit(0 if not FAIL else 1)

asyncio.run(main())
