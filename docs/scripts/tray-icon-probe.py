#!/usr/bin/env python3
"""CDP-Probe für den TrayIcon (t_b3faf678) auf feature/tray-icon.

Belegt mit funktionalen DOM-Assertions (der Rust-Tray selbst ist live
verifiziert: StatusNotifierWatcher-Registrierung + dbusmenu-Layout
[Overlay ein-/ausblenden / Config öffnen / Beenden] + dbusmenu-Event-Clicks,
siehe Task-Report):

  T1 — Frontend registriert den 'tray-open-config'-Listener (Tray→UI-Draht).
  T2 — Simuliertes Tray-Event 'Config öffnen' öffnet die pane-config (gleiches
       Verhalten wie der Zahnrad-Button).
  T3 — ESC im Config-View ruft overlay_config_closed auf (Backend verbirgt das
       Fenster wieder, wenn es nur für die Config gezeigt wurde).
  T4 — Regression: Zahnrad-Button öffnet den Config-View weiterhin.

Datenquelle: smoke-stub.js (canned IPC) + Probe-Wrapper, der invoke-Befehle
und event-Listener-Händeführt aufzeichnet. Screenshots nach docs/screens/.

Self-contained: startet den statischen Server (build/) und headless Chromium.
"""
import asyncio, base64, json, os, pathlib
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()

# Wrapper NACH dem Stub injizieren: zeichnet alle invoke-Befehle und alle
# event-listener-Handler auf (der Stub reicht den Handler als Funktion durch,
# da transformCallback identitär ist) und stellt sie für die Simulation bereit.
WRAP = """
window.__probe = { invokes: [], listeners: {} };
(function(){
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    window.__probe.invokes.push(cmd);
    if (cmd === 'plugin:event|listen' && args && args.event) {
      window.__probe.listeners[args.event] = args.handler;
    }
    return orig(cmd, args);
  };
})();
"""
STUB = STUB + WRAP

OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
URL = os.environ.get("PROBE_URL", "http://127.0.0.1:8796/")

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

async def main():
    # ---- static server (self-contained, lives for this run) ----
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8796", "--bind", "127.0.0.1",
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
        raise RuntimeError("static server on 8796 did not come up")
    print("static server up on 8796")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9228", "--user-data-dir=/tmp/cdp-tray-icon",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9228/json/list", timeout=1) as r:
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

        console_events = []
        async def recv_loop():
            async for raw in cdp:
                msg = json.loads(raw)
                if "id" in msg and msg["id"] in pending:
                    pending[msg["id"]].set_result(msg)
                elif msg.get("method") == "Runtime.exceptionThrown":
                    d = msg["params"]["exceptionDetails"]
                    console_events.append("EXC: " + (d.get("text") or "") + " " + (d.get("exception", {}).get("description", "") or "")[:300])
                elif msg.get("method") == "Log.entryAdded":
                    e = msg["params"]["entry"]
                    console_events.append("LOG: " + e.get("text", "")[:200])
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
            v = r.get("result", {}).get("value")
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return v

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = base64.b64decode(r["data"])
            (OUT / name).write_bytes(data)
            print("  shot:", OUT / name, len(data), "bytes")

        async def key_esc():
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": "Escape", "code": "Escape",
                                                  "windowsVirtualKeyCode": 27, "nativeVirtualKeyCode": 27})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": "Escape", "code": "Escape",
                                                  "windowsVirtualKeyCode": 27, "nativeVirtualKeyCode": 27})
            await asyncio.sleep(0.3)

        # ---------- T1: Listener registriert ----------
        await js("window.__probePromise = null; 0")
        listeners = await js("Object.keys(window.__probe.listeners)")
        check("T1 'tray-open-config'-Listener registriert", "tray-open-config" in (listeners or []),
              f"listeners={listeners}")

        # ---------- T2: Tray-Event öffnet Config-View ----------
        sim = await js("""
            (async function(){
              const h = window.__probe.listeners['tray-open-config'];
              if (typeof h !== 'function') return 'NO HANDLER: ' + typeof h;
              h({ event: 'tray-open-config', id: 0, payload: null });
              await new Promise(r => setTimeout(r, 300));
              const pane = document.querySelector('.pane-config');
              return pane ? 'OPEN' : 'MISSING';
            })()
        """)
        check("T2 Tray-Event 'Config öffnen' öffnet pane-config", sim == "OPEN", str(sim))
        await shot("tray-15-config-via-tray.png")

        # ---------- T3: ESC schließt + overlay_config_closed ----------
        await key_esc()
        closed = await js("!document.querySelector('.pane-config')")
        invokes = await js("window.__probe.invokes")
        check("T3 ESC schließt Config-View", closed is True)
        check("T3 overlay_config_closed wurde invoked",
              bool(invokes) and "overlay_config_closed" in invokes, f"invokes={invokes[-8:]}")

        # ---------- T4: Zahnrad-Regression ----------
        gear = await js("""
            (async function(){
              const b = document.querySelector('button.gear');
              if (!b) return 'NO GEAR BUTTON';
              b.click();
              await new Promise(r => setTimeout(r, 300));
              return document.querySelector('.pane-config') ? 'OPEN' : 'MISSING';
            })()
        """)
        check("T4 Zahnrad-Button öffnet Config-View weiterhin", gear == "OPEN", str(gear))

        if console_events:
            print("  console:", console_events[:6])

    proc.kill()
    server.kill()

    print(f"\nRESULT: {len(PASS)} passed, {len(FAIL)} failed")
    if FAIL:
        print("FAILED:", FAIL)
        raise SystemExit(1)

asyncio.run(main())
