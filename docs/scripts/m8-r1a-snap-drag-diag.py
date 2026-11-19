#!/usr/bin/env python3
"""M8-R1-A Snap-Drag-Diagnose (User-Befund: „snap geht nicht so wirklich").

Diagnostiziert den ECHTEN Drag-Fall: Maus drücken → ziehen → loslassen
(CDP Input.dispatchMouseEvent). Der bestehende Probe (m8-r1a-material-
quality-probe.py) setzt den Regler nur programmatisch (setMat → value +
input-Event) und kann den Drag-Bug deshalb nicht sehen.

Szenario (Stub wie im R1-A-Probe): Agricium-Snaps [346, 588, 1000].
Drag auf ~65 % der Reglerbreite → raw ~650 → nächster Snap = 588.
Gemessen wird NACH dem Loslassen:
  - DOM value des Inputs (Thumb-Position, was der User sieht)
  - cmq-val Text (State, was die Rechnung nutzt)
Bug-Bild: val=588 (State snapt) aber DOM value ~650 (Thumb hängt frei).
"""
import asyncio, json, os, pathlib, shutil, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/cache/scratch/snap-diag")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8899/"

import sys
sys.path.insert(0, str(ROOT / "docs/scripts"))
# WRAP + Helfer aus dem R1-A-Probe übernehmen (Stub-Overrides identisch).
import importlib.util
spec = importlib.util.spec_from_file_location("r1a", ROOT / "docs/scripts/m8-r1a-material-quality-probe.py")
# probe.py führt asyncio.run(main()) auf Modulebene aus — nicht importierbar.
# Deshalb: WRAP direkt aus der Datei extrahieren (zwischen WRAP = r""" und """).
src = (ROOT / "docs/scripts/m8-r1a-material-quality-probe.py").read_text()
start = src.index('WRAP = r"""') + len('WRAP = r"""')
end = src.index('"""', start)
WRAP = src[start:end]

PASS = 0
FAIL = 0
def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS {name}")
    else:
        FAIL += 1
        print(f"  FAIL {name}  {detail}")

shutil.rmtree("/tmp/cdp-snap-diag", ignore_errors=True)


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8899", "--bind", "127.0.0.1",
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
        "--window-size=1600,1000", "--hide-scrollbars",
        "--remote-debugging-port=9246", "--user-data-dir=/tmp/cdp-snap-diag",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9246/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        raise RuntimeError("CDP page target did not come up")

    async with websockets.connect(ws, max_size=None) as cdp:
        mid = {"n": 0}
        pending = {}

        async def call(method, params=None):
            mid["n"] += 1
            cur = mid["n"]
            fut = asyncio.get_event_loop().create_future()
            pending[cur] = fut
            await cdp.send(json.dumps({"id": cur, "method": method, "params": params or {}}))
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

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return None
            return r.get("result", {}).get("value")

        async def wait_for(expr, timeout=10):
            end = asyncio.get_event_loop().time() + timeout
            while asyncio.get_event_loop().time() < end:
                v = await js(expr)
                if v:
                    return v
                await asyncio.sleep(0.15)
            return None

        async def mouse(x, y, type_, btn="left", clicks=1):
            await call("Input.dispatchMouseEvent", {
                "type": type_, "x": x, "y": y, "button": btn,
                "buttons": 1 if type_ == "mouseMoved" else 0,
                "clickCount": clicks})

        async def drag_slider(idx, frac):
            """Echter Drag: Thumb der Zeile idx auf frac (0..1) der Breite ziehen."""
            geo = await js(f"""(() => {{
              const s = document.querySelectorAll('.crafting .cmq-slider')[{idx}];
              const inp = s.querySelector('.cmq-range');
              inp.scrollIntoView({{ block: 'center' }});
              const r = inp.getBoundingClientRect();
              return {{ x: r.left, y: r.top + r.height / 2, w: r.width, h: r.height }};
            }})()""")
            assert geo and geo["w"] > 0, f"no slider geometry: {geo}"
            # Thumb bei value 0 sitzt am linken Rand (Thumb-Hälfte einrechnen).
            tx = geo["x"] + 8
            ty = geo["y"]
            await mouse(tx, ty, "mousePressed", clicks=1)
            await asyncio.sleep(0.1)
            target = geo["x"] + geo["w"] * frac
            await mouse(int(target), ty, "mouseMoved")
            await asyncio.sleep(0.15)
            await mouse(int(target), ty, "mouseReleased")
            await asyncio.sleep(0.3)
            return geo

        await (asyncio.create_subprocess_exec  # stub in Seite injizieren
               ("true"))  # Platzhalter — wrap passiert unten
        async def wrap(expr):
            return ok(await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB + "\n" + expr}))

        await wrap(WRAP)
        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("openCraft('Quality Cannon')")
        await wait_for("craftDetailOpen() && sliderRows().length === 2", 8)

        # Snap-Toggle muss AN sein (Default an). Falls nicht: anmachen.
        rows = await js("sliderRows()")
        if rows and rows[0]["snapOn"] is False:
            await js("toggleSnapAt(0)")
            await asyncio.sleep(0.2)
        await js("setMat(0, 0)")
        await asyncio.sleep(0.2)

        # ---- Echter Drag auf ~60 % → raw ~600 → nächster Snap = 588 ----
        geo = await drag_slider(0, 0.60)
        await asyncio.sleep(0.2)
        rows = await js("sliderRows()")
        dom_val = await js("document.querySelectorAll('.crafting .cmq-range')[0].value")
        print(f"  drag-geo={geo} raw-expected~600  DOM-value={dom_val}  state-val={rows[0]['val'] if rows else None}")
        check("D1 State snapt auf 588 (Rechnung korrekt)",
              rows and rows[0]["val"] == "588", str(rows and rows[0]["val"]))
        check("D2 Thumb/DOM-Wert snapt ebenfalls (User-Sicht)",
              dom_val == "588", f"DOM={dom_val} ( erwartet 588)")

        # ---- ZWEITER Drag in denselben Bucket (0.65 → raw ~650 → 588):
        #      der State ändert sich NICHT (schon 588) — hier war früher der
        #      Bug: der Thumb blieb an der Zeigerposition hängen (User-Befund
        #      „Werte snappen, Regler nicht"). ----
        await drag_slider(0, 0.65)
        await asyncio.sleep(0.2)
        rows = await js("sliderRows()")
        dom_val = await js("document.querySelectorAll('.crafting .cmq-range')[0].value")
        print(f"  zweiter Drag (gleich Bucket): DOM-value={dom_val} state-val={rows[0]['val'] if rows else None}")
        check("D3 Zweiter Drag im selben Bucket: Thumb rastet trotzdem auf 588",
              dom_val == "588" and rows and rows[0]["val"] == "588",
              f"DOM={dom_val} val={rows[0]['val'] if rows else None}")
        data = ok(await call("Page.captureScreenshot", {"format": "png"}))["data"]
        (OUT / "snap-drag-after.png").write_bytes(__import__("base64").b64decode(data))
        print(f"  shot {OUT / 'snap-drag-after.png'}")

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
