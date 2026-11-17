#!/usr/bin/env python3
"""CDP-Probe für t_cf1224c5 (M7-F3, Punkt 2): Sammel-Signatur-Pin im
Pins-only — Renderer „scan-signatures“ (PinScanSignatures) zeigt pro Erz
Kürzel + Basis-Signatur + Multiplikatoren ×1–×5 (Basis·n).

Muster: m7-k3-scan-signature-probe.py (Overlay-State + pins_state gestubbt).
"""
import asyncio, base64, json, os, pathlib, sys
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/m7-f3-redesign")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8798/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

def pin(view_id, box_index, x, y, title, kpi, content, rows=None):
    return {
        "view_id": view_id, "screen": "DP-3", "box_index": box_index,
        "opacity": 100, "pinned": True, "x": x, "y": y,
        "title": title, "kpi": kpi,
        "rows": rows if rows is not None else [],
        "content": content,
    }

COLLECT = pin("mining", 9903, 400, 200, "Scan signatures", None, {
    "type": "scan-signatures",
    "entries": [
        {"oreId": "beryl_raw", "label": "Beryl (Raw)", "value": 3540},
        {"oreId": "gold_ore", "label": "Gold (Ore)", "value": 3885},
    ],
}, rows=[
    {"idx": "0", "name": "Be", "sub": "Beryl (Raw)", "value": "3540", "side": None},
    {"idx": "1", "name": "Go", "sub": "Gold (Ore)", "value": "3885", "side": None},
])

WRAP_TEMPLATE = """
(function() {
  window.__m7f3 = { overlay: %s };
  window.__m7f3.pins = { enabled: true, game_running: true,
    overlay_visible: window.__m7f3.overlay.visible,
    spotlight_screen: 'DP-3', pins: [%s] };
  const orig = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    globalThis.__lastInvoke = { cmd, args: args || {} };
    if (cmd === 'overlay_get_state') return window.__m7f3.overlay;
    if (cmd === 'pins_state') return window.__m7f3.pins;
    return orig(cmd, args);
  };
})();
"""

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

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1500", "--hide-scrollbars",
        "--remote-debugging-port=9239", "--user-data-dir=/tmp/cdp-m7f3-pins",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9239/json/list", timeout=1) as r:
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
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1500, "deviceScaleFactor": 2, "mobile": False})

        async def navigate(extra):
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": extra})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.4)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", OUT / name)

        PINS_ONLY = {"visible": False, "game_running": True, "click_through": False}
        wrap = WRAP_TEMPLATE % (json.dumps(PINS_ONLY), json.dumps(COLLECT))
        await navigate(wrap)

        cls = await js("document.querySelector('main').classList.contains('pins-only')")
        check("pins-only-mode", cls is True, str(cls))
        n = await js("document.querySelectorAll('main .pin').length")
        check("collect-pin-rendered", n == 1, f"pins={n}")
        rows = await js("[...document.querySelectorAll('main .sigs-row')].length")
        check("two-entry-rows", rows == 2, f"rows={rows}")
        # Beryl 3540: ×1..×5 = 3540 7080 10620 14160 17700
        beryl = await js("[...document.querySelectorAll('main .sigs-row')][0].textContent.replace(/\\s+/g,' ')")
        check("beryl-multiples-x1-x5",
              all(v in beryl for v in ["3540", "7080", "10620", "14160", "17700"]),
              beryl)
        gold = await js("[...document.querySelectorAll('main .sigs-row')][1].textContent.replace(/\\s+/g,' ')")
        check("gold-multiples-x1-x5",
              all(v in gold for v in ["3885", "7770", "11655", "15540", "19425"]),
              gold)
        abbrs = await js("[...document.querySelectorAll('main .sigs-abbr')].map(function(e){return e.textContent;}).join(',')")
        check("kuerzel-in-pin", abbrs == "Be,Go", abbrs)
        pe = await js("getComputedStyle(document.querySelector('main .pin')).pointerEvents")
        check("pins-only-display-only", pe == "none", f"pointer-events={pe}")
        await shot("06-pins-only-collect-pin.png")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

asyncio.run(main())
