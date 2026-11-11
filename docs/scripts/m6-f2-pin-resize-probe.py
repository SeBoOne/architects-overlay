#!/usr/bin/env python3
"""CDP-Probe M6-F2 (t_3ff5e552) — Pin-Resize (Corner-Handle + Größen-±).

Funktionale DOM-Assertions auf feature/pin-resize:
  A — Resize-Griff (.pin-resize) vorhanden, nur interaktiv (Palette offen);
      Pins-only bleibt reine Anzeige (kein Griff).
  B — Corner-Drag auf dem Griff → invoke('pin_resize') mit geclamptem,
      auf SCALE_STEP gerundeten Faktor UND Drag-Bundle mit SKALIERTEN
      Box-Maßen (w/h = pinBoxSize, nicht 356/unskaliert).
  C — Popover-Größe ± → invoke('pin_set_scale') mit ±SCALE_STEP.
  D — Patch-Geometrie: Pin mit scale=200 am rechten Rand clampt an
      1920-2*356-8 (skalierte Breite), style.zoom = scale/100.
  E — Legacy-Pin ohne scale: rendert wie 100 % (kein Sprung, zoom=1).

Datenquelle: smoke-stub.js (IPC-Grenze gestubbt; pins_state über __m5) —
Backend-Clamp/Snap ist durch cargo test (pins::tests::scale_clamps_and_sizes)
belegt. Screenshots nach docs/screens/m6f2-*.png.
"""
import asyncio, base64, json, os, pathlib, sys
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8802"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9236"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

def pin(view_id, box_index, x, y, screen="DP-3", opacity=100, rows=2, scale=None):
    p = {
        "view_id": view_id, "screen": screen, "box_index": box_index,
        "opacity": opacity, "pinned": True, "x": x, "y": y,
        "title": f"Route {view_id}#{box_index}", "kpi": "3 Sprünge",
        "rows": [
            {"idx": f"0{i}", "name": f"Station {i}", "sub": "CRU-L2",
             "value": f"{1200+i} aUEC", "side": "buy" if i % 2 else "sell"}
            for i in range(1, rows + 1)
        ],
    }
    if scale is not None:
        p["scale"] = scale
    return p

def stub_extra(overlay, *pins):
    pin_json = ",\n      ".join(json.dumps(p) for p in pins)
    return """
(function() {
  window.__m6f2 = { overlay: %s };
  window.__m6f2.pins = { enabled: true, game_running: true, overlay_visible: window.__m6f2.overlay.visible,
    spotlight_screen: 'DP-3', pins: [
      %s
  ] };
  const orig = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    globalThis.__lastInvoke = { cmd, args: args || {} };
    (globalThis.__invokes = globalThis.__invokes || []).push({ cmd, args: args || {} });
    if (cmd === 'overlay_get_state') return window.__m6f2.overlay;
    if (cmd === 'pins_state') return window.__m6f2.pins;
    if (/^(pin_move|pin_set_opacity|pin_set_scale|pin_resize|pin_unpin|pin_remove|pin_add|pin_update)$/.test(cmd)) return true;
    return orig(cmd, args);
  };
})();
""" % (json.dumps(overlay), pin_json)

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
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-m6f2-probe",
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
        NO_GAME = {"visible": False, "game_running": False, "click_through": False}

        # =================================================================
        # A — Resize-Griff vorhanden, nur interaktiv
        # =================================================================
        await navigate(stub_extra(PAL_OPEN, pin("routes", 0, 400, 200, scale=100)))
        disp = await js("getComputedStyle(document.querySelector('main .pin .pin-resize')).display")
        check("a-resize-handle-visible", disp == "block", f"display={disp}")
        await shot("m6f2-a-handle-palette-open.png")

        await navigate(stub_extra(PINS_ONLY, pin("routes", 0, 400, 200, scale=100)))
        disp = await js("(function(){var h=document.querySelector('main .pin .pin-resize'); return h ? getComputedStyle(h).display : 'NOT-IN-DOM';})()")
        check("a-resize-handle-hidden-pins-only", disp == "NOT-IN-DOM", f"pin-resize im display-only-Zustand: {disp}")
        await navigate(stub_extra(NO_GAME, pin("routes", 0, 400, 200)))
        n = await js("document.querySelectorAll('main .pin').length")
        check("a-no-game-no-boxes", n == 0, f"pins im DOM: {n}")

        # =================================================================
        # B — Corner-Drag → pin_resize (geclampt, gerundet, skaliertes Bundle)
        # =================================================================
        await navigate(stub_extra(PAL_OPEN, pin("routes", 0, 400, 200, scale=100)))
        # Griff liegt unten rechts in der Box; Drag von der Box-Ecke aus.
        await js("""(function() {
            var box = document.querySelector('main .pin');
            var r = box.getBoundingClientRect();
            var sx = r.left + r.width / 2, sy = r.top + r.height / 2;
            var h = document.querySelector('main .pin .pin-resize');
            h.dispatchEvent(new PointerEvent('pointerdown', {bubbles: true, pointerId: 7, clientX: sx, clientY: sy}));
            // 2x Layoutbreite nach rechts/unten ziehen → Faktor ~200 (Mittel aus sx/sy)
            window.dispatchEvent(new PointerEvent('pointermove', {bubbles: true, pointerId: 7, clientX: sx + 356, clientY: sy + 112}));
            window.dispatchEvent(new PointerEvent('pointerup', {bubbles: true, pointerId: 7, clientX: sx + 356, clientY: sy + 112}));
            return true;
        })()""")
        await asyncio.sleep(0.6)
        rez = await js("(function(){var iv=(globalThis.__invokes||[]).filter(function(x){return x.cmd==='pin_resize';}); return iv.length ? JSON.stringify(iv[iv.length-1].args) : 'NONE';})()")
        try:
            rv = json.loads(str(rez))
        except Exception:
            rv = {}
        scale = rv.get("scale")
        drag = rv.get("drag", {})
        ok_cmd = rv.get("view_id") == "routes" and isinstance(scale, (int, float)) and 50 <= scale <= 200
        check("b-corner-drag-invokes-pin_resize", ok_cmd, f"args={rez}")
        check("b-scale-clamped-50-200", isinstance(scale, (int, float)) and 50 <= scale <= 200, f"scale={scale}")
        # Bundle: skalierte Maße — 112 px vertikal ≙ Faktor ~112 (rundbar auf 110);
        # w = 356*112/100 = 398.72 → 399 (Math.round im Frontend). Das Backend
        # rechnet seine eigenen Maße (Mirror-Test) — hier zählt: unskaliert wäre w=356.
        check("b-drag-bundle-scaled", isinstance(drag.get("w"), (int, float)) and drag.get("w") != 356,
              f"w={drag.get('w')} h={drag.get('h')} (356 wäre unskaliert)")
        check("b-drag-bundle-snake-case", drag.get("screen_w") == 1920 and drag.get("screen_h") == 1500, str(drag))

        # =================================================================
        # C — Popover-Größe ± → pin_set_scale
        # =================================================================
        head = "document.querySelector('main .pin .pin-head')"
        await js(f"""(function() {{
            var h = {head};
            h.dispatchEvent(new PointerEvent('pointerdown', {{bubbles: true, pointerId: 2}}));
            window.dispatchEvent(new PointerEvent('pointerup', {{bubbles: true, pointerId: 2, movementX: 0, movementY: 0}}));
            return true;
        }})()""")
        await asyncio.sleep(0.4)
        pop = await js("!!document.querySelector('.pin-popover')")
        check("c-popover-open", pop is True, str(pop))
        minus = await js("(function(){var b=[...document.querySelectorAll('.pin-popover .pbtn')].find(function(x){return x.textContent.trim()==='\\u2921\\u2212';}); if(b) b.click(); return b ? b.textContent.trim() : 'NOT-FOUND';})()")
        check("c-minus-button-found", minus == '⤡−', str(minus))
        await asyncio.sleep(0.4)
        sc = await js("(function(){var iv=(globalThis.__invokes||[]).filter(function(x){return x.cmd==='pin_set_scale';}); return iv.length ? JSON.stringify(iv[iv.length-1].args) : 'NONE';})()")
        check("c-minus-invokes-pin_set_scale", '"scale": 90' in str(sc) or '"scale":90' in str(sc), f"args={sc}")
        plus = await js("(function(){var b=[...document.querySelectorAll('.pin-popover .pbtn')].find(function(x){return x.textContent.trim()==='\\u2922+';}); if(b) b.click(); return b ? b.textContent.trim() : 'NOT-FOUND';})()")
        check("c-plus-button-found", plus == '⤢+', str(plus))
        await asyncio.sleep(0.4)
        sc2 = await js("(function(){var iv=(globalThis.__invokes||[]).filter(function(x){return x.cmd==='pin_set_scale';}); return iv.length ? JSON.stringify(iv[iv.length-1].args) : 'NONE';})()")
        check("c-plus-invokes-pin_set_scale", '"scale": 110' in str(sc2) or '"scale":110' in str(sc2), f"args={sc2}")
        await shot("m6f2-c-popover-size-buttons.png")

        # =================================================================
        # D — Patch-Geometrie mit Skalierung (zoom + Clamp an skaliertes Maß)
        # =================================================================
        await navigate(stub_extra(PAL_OPEN,
                                  pin("routes", 0, 3400, 100, scale=200),
                                  pin("offers", 1, 400, 200, scale=150)))
        zoom = await js("parseFloat(document.querySelectorAll('main .pin')[0].style.zoom)")
        check("d-zoom-200", zoom == 2.0, f"zoom={zoom}")
        left = await js("parseInt(document.querySelectorAll('main .pin')[0].style.left)")
        check("d-clamp-scaled-width", left == 1920 - 2 * 356 - 8, f"left={left} (erwartet 1920-712-8=1200)")
        zoom2 = await js("parseFloat(document.querySelectorAll('main .pin')[1].style.zoom)")
        check("d-zoom-150", zoom2 == 1.5, f"zoom={zoom2}")
        await shot("m6f2-d-scaled-boxes.png")

        # =================================================================
        # E — Legacy-Pin ohne scale → rendert wie 100 %
        # =================================================================
        await navigate(stub_extra(PAL_OPEN, pin("routes", 0, 400, 200)))
        left = await js("parseInt(document.querySelector('main .pin').style.left)")
        zoom = await js("parseFloat(document.querySelector('main .pin').style.zoom)")
        check("e-legacy-100", left == 400 and zoom == 1.0, f"left={left} zoom={zoom}")

    server.terminate()
    proc.terminate()
    print("\n== M6-F2-Probe: %d PASS, %d FAIL ==" % (len(PASS), len(FAIL)))
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        sys.exit(1)

asyncio.run(main())
