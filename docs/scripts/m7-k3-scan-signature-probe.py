#!/usr/bin/env python3
"""CDP-Probe M7-K3 (t_cfea5bea) — Pin-Content-Typ „scan-signature".

Funktionale DOM-Assertions auf feature/m7-k3-scan-signature-pins:
  A — Palette offen: Pin mit content {type:'scan-signature', label, value,
      tier, max} rendert den Registry-Renderer (.sig): Label, Wert, Tier-
      Badge T3 und Orientierungsbalken width = value/max (42 % bei 420/1000).
  B — Null-Signatur: value null → Wert „—" (Pin bleibt), KEIN Balken.
  C — Rows-Fallback unverändert: Pin OHNE content rendert weiterhin den
      rows-Snapshot (Registry-Erweiterung bricht Bestand nicht).
  D — Pins-only (Palette verborgen): scan-signature-Box bleibt reine
      Anzeige sichtbar (gleiche Sichtbarkeitskette wie alle Pins).

Datenquelle: smoke-stub.js (IPC-Grenze gestubbt, pins_state über __m7k3-
Override) — die Backend-Seite (pin_add content-JSON passthrough) ist durch
pins.rs-Tests belegt; hier Frontend-Rendering. Screenshots nach docs/screens/m7-k3-*.png.

Self-contained: startet statischen Server (build/) + headless Chromium.
"""
import asyncio, base64, json, os, pathlib, sys
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8809"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9243"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

def pin(view_id, box_index, x, y, title, kpi, content, rows=None):
    return {
        "view_id": view_id, "screen": "DP-3", "box_index": box_index,
        "opacity": 100, "pinned": True, "x": x, "y": y,
        "title": title, "kpi": kpi,
        "rows": rows if rows is not None else [],
        "content": content,
    }

def stub_extra(overlay):
    """__TAURI_INTERNALS__-Wrapper: pins_state aus __m7k3."""
    scan_full = pin("mining", 0, 400, 200, "QUANTAINIUM", None,
                    {"type": "scan-signature", "label": "Quantainium",
                     "value": 420, "tier": 3, "max": 1000})
    scan_null = pin("mining", 1, 400, 480, "TUNGSTEN", None,
                    {"type": "scan-signature", "label": "Tungsten",
                     "value": None, "tier": None, "max": None})
    rows_pin = pin("routes", 2, 900, 200, "Route 07", "3 Sprünge", None,
                   rows=[{"idx": "01", "name": "Station A", "sub": "CRU-L2",
                          "value": "1200 aUEC", "side": "buy"}])
    return """
(function() {
  window.__m7k3 = { overlay: %s };
  window.__m7k3.pins = { enabled: true, game_running: true,
    overlay_visible: window.__m7k3.overlay.visible,
    spotlight_screen: 'DP-3', pins: [
      %s,
      %s,
      %s
  ] };
  const orig = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    globalThis.__lastInvoke = { cmd, args: args || {} };
    (globalThis.__invokes = globalThis.__invokes || []).push({ cmd, args: args || {} });
    if (cmd === 'overlay_get_state') return window.__m7k3.overlay;
    if (cmd === 'pins_state') return window.__m7k3.pins;
    if (/^(pin_move|pin_set_opacity|pin_unpin|pin_remove|pin_add|pin_update|pin_set_scale|pin_resize)$/.test(cmd)) return true;
    return orig(cmd, args);
  };
})();
""" % (
        json.dumps(overlay),
        json.dumps(scan_full),
        json.dumps(scan_null),
        json.dumps(rows_pin),
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
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-m7k3-probe",
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

        # =================================================================
        # A — Palette offen: scan-signature-Renderer (Registry-Wahl)
        # =================================================================
        await navigate(stub_extra(PAL_OPEN))
        n = await js("document.querySelectorAll('main .pin').length")
        check("a-three-pins-rendered", n == 3, f"pins im DOM: {n}")
        sig = await js("!!document.querySelector('main .pin .sig')")
        check("a-sig-renderer-used", sig is True, f".sig present={sig}")
        label = await js("document.querySelector('main .pin .sig-label')?.textContent")
        check("a-label-ore-name", label == "Quantainium", str(label))
        value = await js("document.querySelector('main .pin .sig-value')?.textContent")
        check("a-value-number", value == "420", str(value))
        tier = await js("document.querySelector('main .pin .sig-tier')?.textContent")
        check("a-tier-badge", tier == "T3", str(tier))
        barw = await js("document.querySelector('main .pin .sig-bar i')?.style.width")
        check("a-bar-42pct-of-chart-max", barw == "42%", str(barw))
        pe = await js("getComputedStyle(document.querySelector('main .pin')).pointerEvents")
        check("a-interactive-palette-open", pe == "auto", f"pointer-events={pe}")
        await shot("m7-k3-a-palette-scan-signature.png")

        # =================================================================
        # B — value null → „—", kein Balken (Pin bleibt trotzdem)
        # =================================================================
        null_pin = await js("[...document.querySelectorAll('main .pin')].find(function(p){return p.textContent.indexOf('Tungsten') >= 0;})")
        check("b-null-pin-pinned", null_pin is not None, "null-Signatur gerendert")
        nv = await js("[...document.querySelectorAll('main .pin')].find(function(p){return p.textContent.indexOf('Tungsten')>=0;})?.querySelector('.sig-value')?.textContent")
        check("b-null-value-dash", nv == "—", str(nv))
        nb = await js("[...document.querySelectorAll('main .pin')].find(function(p){return p.textContent.indexOf('Tungsten')>=0;})?.querySelector('.sig-bar')")
        check("b-null-no-bar", nb is None, f"bar={nb is not None}")
        nt = await js("[...document.querySelectorAll('main .pin')].find(function(p){return p.textContent.indexOf('Tungsten')>=0;})?.querySelector('.sig-tier')")
        check("b-null-no-tier-badge", nt is None, f"tier={nt is not None}")

        # =================================================================
        # C — Rows-Fallback unverändert (Pin ohne content)
        # =================================================================
        rows_ok = await js("[...document.querySelectorAll('main .pin')].some(function(p){return p.querySelector('.pin-step') && p.querySelector('.pin-step .nm')?.textContent.indexOf('Station A') >= 0;})")
        check("c-rows-fallback-intact", rows_ok is True, str(rows_ok))
        no_sig_in_rows = await js("[...document.querySelectorAll('main .pin')].every(function(p){return p.querySelector('.pin-step') ? !p.querySelector('.sig') : true;})")
        check("c-rows-pin-has-no-sig", no_sig_in_rows is True, str(no_sig_in_rows))

        # =================================================================
        # D — Pins-only: Boxen reine Anzeige, scan-signature sichtbar
        # =================================================================
        await navigate(stub_extra(PINS_ONLY))
        cls = await js("document.querySelector('main').classList.contains('pins-only')")
        check("d-main-pins-only", cls is True, str(cls))
        n = await js("document.querySelectorAll('main .pin').length")
        check("d-boxes-visible", n == 3, f"pins im DOM: {n}")
        sig = await js("!!document.querySelector('main .pin .sig')")
        check("d-sig-visible-display-only", sig is True, f".sig present={sig}")
        pe = await js("getComputedStyle(document.querySelector('main .pin')).pointerEvents")
        check("d-display-only-pointer-events", pe == "none", f"pointer-events={pe}")
        pop = await js("!!document.querySelector('.pin-popover')")
        check("d-no-popover", pop is False, str(pop))
        await shot("m7-k3-d-pins-only-scan-signature.png")

    server.terminate()
    proc.terminate()
    print("\n== M7-K3-Probe: %d PASS, %d FAIL ==" % (len(PASS), len(FAIL)))
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        sys.exit(1)

asyncio.run(main())
