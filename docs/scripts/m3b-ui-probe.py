#!/usr/bin/env python3
"""CDP-Probe M3b (t_d9c09acf) — Fenstermodus-Feinschliff.

Funktionale DOM-Assertions auf feat/fenstermodus-m3:
  A — Titelband-Menü-Endstand (Mockup B): Show overlay (Hotkey-Label aus
      cfg.shortcut_toggle), Toggle click-through, Show/hide window mode,
      Settings… F12, Help / About (docked an Info-Popup), Quit rot.
  B — Menü-IPC-Pfade via window.__invokes (Stub loggt JEDEN invoke):
      overlay_set_visible visible:true; toggleClickThrough →
      overlay_get_state + overlay_set_click_through ignore flip;
      toggleWindowMode → window_toggle.
  C — Titelband-Mode-Sync: Backend-Event ao:window-visibility (true) schaltet
      den Mode-Umschalter auf 'window' (Tray-Re-Show, M1-R1 Befund a).
  D — Live-Sprachwechsel im Fenstermodus: ein EINZIGER persistent
      registrierter Bootstrap-Script liest die Sprache aus
      localStorage['__probeLang'] (default en) → deterministische de-Runde
      ohne Neustart (i18n-Store + Stub-Settings). Kein
      Angebot-Neustart-Fallback: die Labels stehen nach Reload.

Datenquelle: smoke-stub.js (IPC-Grenze gestubbt; Rust-Seite durch
cargo tests in tray.rs::tests + window.rs belegt). Screenshots nach
docs/screens/m3b-*.png.

Self-contained: startet statischen Server (build/) + headless Chromium.
"""
import asyncio, base64, json, os, pathlib, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8796"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9230"))
URL = f"http://127.0.0.1:{PORT}/#/window"

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
        "--window-size=1200,900", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-m3b-probe-{os.getpid()}",
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
        await call("Emulation.setDeviceMetricsOverride", {"width": 1200, "height": 900, "deviceScaleFactor": 2, "mobile": False})

        # M3b: EINER Basis-Script — der Stub selbst liest die Sprache aus
        # localStorage['__probeLang'] (Default en), so bleibt die de-Runde
        # deterministisch (keine addScriptToEvaluateOnNewDocument-Akkumulation).
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.2)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            v = r.get("result", {}).get("value")
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return v

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = base64.b64decode(r["data"])
            (OUT / name).write_bytes(data)
            print("  shot:", OUT / name, len(data), "bytes")

        async def open_menu():
            """Menü Toggle-sicher öffnen: .tb-menu ist menuOpen=!menuOpen."""
            st = await js("document.querySelector('.tb-menu') ? document.querySelector('.tb-menu').getAttribute('aria-expanded') : 'NONE'")
            if st != "true":
                await js("(function(){var b=document.querySelector('.tb-menu'); if(b) b.click();})()")
                await asyncio.sleep(0.3)

        async def click_item(label):
            """Menüpunkt per Label klicken; __invokes für die Assertion leeren."""
            await js("(window.__invokes = []).length = 0;")
            await open_menu()
            return await js(
                "(function(){var b=[...document.querySelectorAll('.tb-pop button')].find(function(x){return x.textContent.indexOf(" + json.dumps(label) + ")>=0;});"
                "if(b){b.click(); return true;} return false;})()")

        # =================================================================
        # A — Titelband-Menü-Endstand (Mockup B)
        # =================================================================
        await shot("m3b-titlebar.png")

        tb = await js("!!document.querySelector('.tb')")
        check("a-titlebar-renders", tb is True)
        mode_btn = await js(
            "JSON.stringify((function(){var b=[...document.querySelectorAll('.tb-mode button')];"
            "return b.map(function(x){return x.textContent.trim() + (x.classList.contains('on') ? ':on' : '');});})())")
        check("a-mode-toggle-initial", "Overlay" in str(mode_btn) and "Window:on" in str(mode_btn), str(mode_btn))

        await open_menu()
        items = await js(
            "JSON.stringify([...document.querySelectorAll('.tb-pop button')].map(function(x){return x.textContent.trim();}))")
        items = str(items)
        check("a-menu-overlay-item", "Show overlay" in items, items)
        check("a-menu-clickthrough-item", "Toggle click-through" in items)
        check("a-menu-window-toggle-item", "Show/hide window mode" in items)
        check("a-menu-settings-item", "Settings" in items and "F12" in items)
        check("a-menu-about-item", "Help / About" in items)
        check("a-menu-quit-item", "Quit" in items)
        hotkey = await js(
            "JSON.stringify((function(){var b=[...document.querySelectorAll('.tb-pop button')].find(function(x){return x.textContent.indexOf('Show overlay')>=0;});"
            "return b ? b.querySelector('.k') ? b.querySelector('.k').textContent.trim() : 'NONE' : 'NONE';})())")
        check("a-menu-overlay-hotkey", "Ctrl+Shift+U" in str(hotkey), str(hotkey))
        await shot("m3b-menu.png")

        # =================================================================
        # B — Menü-IPC-Pfade (window.__invokes gegen Hintergrund-Listen)
        # =================================================================
        clicked = await click_item("Show overlay")
        inv = await js(
            "JSON.stringify((window.__invokes||[]).filter(function(x){return x.cmd === 'overlay_set_visible';}).map(function(x){return x.args.visible;}))")
        check("b-overlay-ipc", clicked is True and "[true]" in str(inv), str(inv))

        clicked = await click_item("Toggle click-through")
        inv = await js(
            "JSON.stringify((window.__invokes||[]).filter(function(x){return x.cmd === 'overlay_set_click_through';}).map(function(x){return x.args.ignore;}))")
        check("b-clickthrough-ipc", clicked is True and "[true]" in str(inv), str(inv))

        clicked = await click_item("Show/hide window mode")
        inv = await js(
            "JSON.stringify((window.__invokes||[]).filter(function(x){return x.cmd === 'window_toggle';}).map(function(x){return x.cmd;}))")
        check("b-window-toggle-ipc", clicked is True and "[\"window_toggle\"]" in str(inv), str(inv))

        # Listener VOR dem Klick registrieren (openInfo dispatcht synchron).
        await js("(function(){window.__infoFired = false; window.addEventListener('ao-window-open-info', function(){window.__infoFired = true;});})()")
        clicked = await click_item("Help / About")
        fired = await js("window.__infoFired")
        check("b-info-event", clicked is True and fired is True, str(fired))

        # =================================================================
        # C — Titelband-Mode-Sync (Tray-Re-Show → Event)
        # =================================================================
        # C1: per Mode-Umschalter (setMode('overlay')) versteckt sich das
        # Fenster; danach zieht das Backend-Event ao:window-visibility=true
        # den Mode zurück auf 'window' (Tray „Fenstermodus zeigen/verstecken").
        await js("(window.__invokes = []).length = 0;")
        await js("(function(){var b=[...document.querySelectorAll('.tb-mode button')].find(function(x){return x.textContent.indexOf('Overlay')>=0;}); if(b){b.click(); return true;} return false;})()")
        await asyncio.sleep(0.15)
        inv = await js("JSON.stringify((window.__invokes||[]).filter(function(x){return x.cmd === 'overlay_set_visible';}).map(function(x){return x.args.visible;}))")
        check("c-overlay-mode-ipc1", "[true]" in str(inv), str(inv))
        await asyncio.sleep(0.3)
        inv = await js("JSON.stringify((window.__invokes||[]).filter(function(x){return x.cmd === 'window_hide';}).map(function(x){return x.cmd;}))")
        check("c-overlay-mode-ipc2-hide", "[\"window_hide\"]" in str(inv), str(inv))
        on = await js("JSON.stringify((function(){var b=[...document.querySelectorAll('.tb-mode button')].find(function(x){return x.classList.contains('on');}); return b ? b.textContent.trim() : 'NONE';})())")
        check("c-mode-overlay-on", "Overlay" in str(on), str(on))

        await js("window.__stubFireTauriEvent('ao:window-visibility', true)")
        await asyncio.sleep(0.4)
        on = await js("JSON.stringify((function(){var b=[...document.querySelectorAll('.tb-mode button')].find(function(x){return x.classList.contains('on');}); return b ? b.textContent.trim() : 'NONE';})())")
        check("c-mode-sync-back", "Window" in str(on), str(on))
        await shot("m3b-mode-sync.png")

        # =================================================================
        # D — Live-Sprachwechsel im Fenstermodus (de)
        # =================================================================
        # Echter Live-Switch-Pfad (T2): Settings öffnen → „Deutsch"-Chip →
        # chooseLanguage('de') → settings_set_language (Stub persistiert) +
        # setLanguage('de') → $t-Store zieht ÜBERALL nach, auch im Titelband.
        await js("window.dispatchEvent(new CustomEvent('ao-window-open-config'))")
        await asyncio.sleep(0.8)
        clicked = await js(
            "(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()==='Deutsch';});"
            "if(b){b.click(); return true;} return false;})()")
        await asyncio.sleep(0.8)
        lang = await js("window.__stubState ? window.__stubState.language : 'NONE'")
        check("d-chooseLanguage-de", clicked is True and lang == "de", str(lang))
        await open_menu()
        items = await js(
            "JSON.stringify([...document.querySelectorAll('.tb-pop button')].map(function(x){return x.textContent.trim();}))")
        items = str(items)
        check("d-menu-de-labels", "Overlay einblenden" in items and "Klick-Durchlass umschalten" in items
              and "Fenstermodus zeigen/verstecken" in items and "Hilfe / Über" in items, items)
        mode_btn = await js(
            "JSON.stringify((function(){var b=[...document.querySelectorAll('.tb-mode button')];"
            "return b.map(function(x){return x.textContent.trim() + (x.classList.contains('on') ? ':on' : '');});})())")
        check("d-mode-de-labels", "Fenster:on" in str(mode_btn), str(mode_btn))
        await shot("m3b-menu-de.png")

    print("\n== M3b-Probe: %d PASS, %d FAIL ==" % (len(PASS), len(FAIL)))
    if FAIL:
        print("FAILURES:", FAIL)
    proc.terminate()
    server.terminate()
    return 0 if not FAIL else 1


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
