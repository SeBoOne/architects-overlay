#!/usr/bin/env python3
"""CDP-Probe M5 (t_d10a0d84) — Pins-in-Spotlight-Refactor (Ein-Fenster-Architektur).

Funktionale DOM-Assertions auf feat/pins-in-spotlight:
  A — Palette offen: Pin-Boxen als DOM im Spotlight (PinBoxes.svelte), nur
      Pins des eigenen Screens (spotlight_screen-Filter), Position x/y
      viewport-absolut, interaktiv (pointer-events:auto), .shell sichtbar.
  B — Interaktion: Drag auf dem Kopf → invoke('pin_move') mit Drag-Bundle;
      Klick (ohne Bewegung) → Mini-Controller-Popover (.pin-popover);
      Preset-Klick → invoke('pin_set_opacity').
  C — Pins-only (Palette verborgen, Pins aktiv): main.pal-root trägt
      pins-only, .shell display:none, Boxen bleiben sichtbar als reine
      Anzeige (pointer-events:none — Fenster complete-through im Backend).
  D — Click-Through bei offener Palette: Boxen gerendert, aber nicht
      interaktiv (pointer-events:none — M4-E-Gate).
  E — Kein Spiel (game_running=false): keine Boxen (§9.1.5 — kein Kanal).

Datenquelle: smoke-stub.js (IPC-Grenze gestubbt; overlay_get_state/pins_state
über __m5-Override) — die Backend-Seite ist durch cargo tests (pins.rs
spotlight_mode, overlay.rs Apply-Visibility-Zweige) belegt. Screenshots nach
docs/screens/m5-*.png.

Self-contained: startet statischen Server (build/) + headless Chromium.
"""
import asyncio, base64, json, os, pathlib, sys
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8801"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9235"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

def pin(view_id, box_index, x, y, screen="DP-3", opacity=100, rows=2):
    return {
        "view_id": view_id, "screen": screen, "box_index": box_index,
        "opacity": opacity, "pinned": True, "x": x, "y": y,
        "title": f"Route {view_id}#{box_index}", "kpi": "3 Sprünge",
        "rows": [
            {"idx": f"0{i}", "name": f"Station {i}", "sub": "CRU-L2",
             "value": f"{1200+i} aUEC", "side": "buy" if i % 2 else "sell"}
            for i in range(1, rows + 1)
        ],
    }

def stub_extra(overlay):
    """__TAURI_INTERNALS__-Wrapper: overlay_get_state/pins_state aus __m5."""
    return """
(function() {
  // IIFE: AddScriptToEvaluateOnNewDocument akkumuliert über Navigations —
  // top-level const würde bei der zweiten Navigation redeclared → SyntaxError.
  window.__m5 = { overlay: %s };
  window.__m5.pins = { enabled: true, game_running: true, overlay_visible: window.__m5.overlay.visible,
    spotlight_screen: 'DP-3', pins: [
      %s,
      %s
  ] };
  const orig = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    // __invokes-Aufzeichnung (smoke-stub-Vertrag) für ALLE Befehle — auch
    // für die hier gestubbten Pin-Commands (Probe-Assertions).
    globalThis.__lastInvoke = { cmd, args: args || {} };
    (globalThis.__invokes = globalThis.__invokes || []).push({ cmd, args: args || {} });
    if (cmd === 'overlay_get_state') return window.__m5.overlay;
    if (cmd === 'pins_state') return window.__m5.pins;
    if (/^(pin_move|pin_set_opacity|pin_unpin|pin_remove|pin_add|pin_update)$/.test(cmd)) return true;
    return orig(cmd, args);
  };
})();
""" % (
        json.dumps(overlay),
        json.dumps(pin("routes", 0, 400, 200)),
        json.dumps(pin("offers", 1, 1200, 500, screen="HDMI-A-2")),
    )

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
        "--window-size=1920,1500", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-m5-probe",
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
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1500, "deviceScaleFactor": 2, "mobile": False})

        async def navigate(extra):
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
            if extra:
                await call("Page.addScriptToEvaluateOnNewDocument", {"source": extra})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.4)

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

        PAL_OPEN = {"visible": True, "game_running": True, "click_through": False}
        PINS_ONLY = {"visible": False, "game_running": True, "click_through": False}
        PAL_CLICK_THROUGH = {"visible": True, "game_running": True, "click_through": True}
        NO_GAME = {"visible": False, "game_running": False, "click_through": False}

        # =================================================================
        # A — Palette offen: Boxen im Spotlight, Screen-Filter, interaktiv
        # =================================================================
        await navigate(stub_extra(PAL_OPEN))
        n = await js("document.querySelectorAll('main .pin').length")
        check("a-box-count-own-screen-only", n == 1, f"pins im DOM: {n} (DP-3-Pin gerendert, HDMI-A-2-Pin gefiltert)")
        pe = await js("getComputedStyle(document.querySelector('main .pin')).pointerEvents")
        check("a-box-interactive", pe == "auto", f"pointer-events={pe}")
        left = await js("parseInt(document.querySelector('main .pin').style.left)")
        top = await js("parseInt(document.querySelector('main .pin').style.top)")
        check("a-box-position-viewport-absolute", left == 400 and top == 200, f"left={left} top={top}")
        shell_disp = await js("getComputedStyle(document.querySelector('main .shell')).display")
        check("a-shell-visible-with-palette", shell_disp != "none", f"shell display={shell_disp}")
        pins_only_cls = await js("document.querySelector('main').classList.contains('pins-only')")
        check("a-main-not-pins-only", pins_only_cls is False, str(pins_only_cls))
        await shot("m5-a-palette-open-with-boxes.png")

        # =================================================================
        # B — Interaktion: Drag → pin_move; Klick → Popover; Preset → opacity
        # =================================================================
        head = "document.querySelector('main .pin .pin-head')"
        await js(f"""(function() {{
            var h = {head};
            h.dispatchEvent(new PointerEvent('pointerdown', {{bubbles: true, pointerId: 1}}));
            window.dispatchEvent(new PointerEvent('pointermove', {{bubbles: true, pointerId: 1, movementX: 60, movementY: 25}}));
            window.dispatchEvent(new PointerEvent('pointerup', {{bubbles: true, pointerId: 1, movementX: 60, movementY: 25}}));
            return true;
        }})()""")
        await asyncio.sleep(0.5)
        move = await js("(function(){var iv=(globalThis.__invokes||[]).filter(function(x){return x.cmd==='pin_move';}); return iv.length ? JSON.stringify(iv[iv.length-1].args) : 'NONE';})()")
        try:
            mv = json.loads(str(move))
        except Exception:
            mv = {}
        drag = mv.get("drag", {})
        check("b-drag-invokes-pin_move", mv.get("view_id") == "routes" and drag.get("x") == 460 and drag.get("y") == 225,
              f"args={move}")
        check("b-drag-bundle-snake-case", drag.get("screen_w") == 1920 and drag.get("screen_h") == 1500, str(drag))

        await js(f"""(function() {{
            var h = {head};
            h.dispatchEvent(new PointerEvent('pointerdown', {{bubbles: true, pointerId: 2}}));
            window.dispatchEvent(new PointerEvent('pointerup', {{bubbles: true, pointerId: 2, movementX: 0, movementY: 0}}));
            return true;
        }})()""")
        await asyncio.sleep(0.4)
        pop = await js("!!document.querySelector('.pin-popover')")
        check("b-click-opens-popover", pop is True, str(pop))
        pop_pe = await js("getComputedStyle(document.querySelector('.pin-popover')).pointerEvents")
        check("b-popover-clickable", pop_pe == "auto", f"pointer-events={pop_pe}")
        await js("(function(){var b=[...document.querySelectorAll('.pin-popover .pbtn')].find(function(x){return x.textContent.trim()==='55';}); if(b) b.click(); return !!b;})()")
        await asyncio.sleep(0.4)
        op = await js("(function(){var iv=(globalThis.__invokes||[]).filter(function(x){return x.cmd==='pin_set_opacity';}); return iv.length ? JSON.stringify(iv[iv.length-1].args) : 'NONE';})()")
        check("b-preset-invokes-pin_set_opacity", '"opacity": 55' in str(op) or '"opacity":55' in str(op), f"args={op}")
        await shot("m5-b-popover-open.png")

        # =================================================================
        # C — Pins-only: Palette-DOM versteckt, Boxen reine Anzeige
        # =================================================================
        await navigate(stub_extra(PINS_ONLY))
        pins_only_cls = await js("document.querySelector('main').classList.contains('pins-only')")
        check("c-main-has-pins-only", pins_only_cls is True, str(pins_only_cls))
        shell_disp = await js("getComputedStyle(document.querySelector('main .shell')).display")
        check("c-shell-display-none", shell_disp == "none", f"shell display={shell_disp}")
        n = await js("document.querySelectorAll('main .pin').length")
        check("c-boxes-still-visible", n == 1, f"pins im DOM: {n}")
        pe = await js("getComputedStyle(document.querySelector('main .pin')).pointerEvents")
        check("c-boxes-display-only", pe == "none", f"pointer-events={pe}")
        pop = await js("!!document.querySelector('.pin-popover')")
        check("c-no-popover", pop is False, str(pop))
        await shot("m5-c-pins-only-display.png")

        # =================================================================
        # D — Click-Through bei offener Palette: gerendert, nicht interaktiv
        # =================================================================
        await navigate(stub_extra(PAL_CLICK_THROUGH))
        n = await js("document.querySelectorAll('main .pin').length")
        pe = await js("getComputedStyle(document.querySelector('main .pin')).pointerEvents")
        shell_disp = await js("getComputedStyle(document.querySelector('main .shell')).display")
        check("d-ct-boxes-not-interactive", n == 1 and pe == "none" and shell_disp != "none",
              f"pins={n} pointer-events={pe} shell={shell_disp}")

        # =================================================================
        # E — Kein Spiel: keine Boxen (§9.1.5)
        # =================================================================
        await navigate(stub_extra(NO_GAME))
        n = await js("document.querySelectorAll('main .pin').length")
        check("e-no-game-no-boxes", n == 0, f"pins im DOM: {n}")

    server.terminate()
    proc.terminate()
    print("\n== M5-Probe: %d PASS, %d FAIL ==" % (len(PASS), len(FAIL)))
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        sys.exit(1)

asyncio.run(main())
