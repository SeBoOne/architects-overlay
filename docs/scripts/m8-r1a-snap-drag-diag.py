#!/usr/bin/env python3
"""M8-R1-A Snap-Drag-Diagnose (User-Befund: „snap geht nicht so wirklich“).

Diagnostiziert den ECHTEN Drag-Fall: Maus drücken → ziehen → loslassen
(CDP Input.dispatchMouseEvent). Der bestehende Probe (m8-r1a-material-
quality-probe.py) setzt den Regler nur programmatisch (setMat → value +
input-Event) und kann den Drag-Bug deshalb nicht sehen.

Band-agnostisch (M8-R2, SCMDB qualityBands): die erwarteten Snap-Punkte
werden aus dem Snap-Titel der Zeile gelesen („only quantized values
[...]“) und der Erwartungswert per nearest berechnet — der Test bricht
nicht mehr, wenn sich die Bands/Quelle ändern.

Szenario:
  D1/D2 Drag auf ~60 % → raw aus dem DOM gelesen (during drag) →
     nearest(Bands) → nach Release müssen State UND Thumb (DOM value)
     auf diesem Punkt stehen.
  D3 ZWEITER Drag in denselben Bucket: State ändert sich evtl. nicht —
     auch dann muss der Thumb auf dem Punkt landen (Controlled-Input-Fall).
"""
import asyncio, json, os, pathlib, re, shutil, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/cache/scratch/snap-diag")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8899/"

# WRAP aus dem R1-A-Probe extrahieren (Stub-Overrides identisch; probe.py
# führt asyncio.run(main()) auf Modulebene aus — nicht importierbar).
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


def nearest(points, value):
    return min(points, key=lambda p: abs(p - value))


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

        async def mouse(x, y, type_, clicks=1):
            await call("Input.dispatchMouseEvent", {
                "type": type_, "x": int(x), "y": int(y), "button": "left",
                "buttons": 1 if type_ in ("mouseMoved", "mousePressed") else 0,
                "clickCount": clicks})

        # Bands aus dem Snap-Titel der Zeile lesen („[346, 500, 588, …]“).
        async def row_bands(idx):
            title = await js(
                f"document.querySelectorAll('.crafting .cmq-snap')[{idx}]?.title || ''")
            m = re.search(r"\[([\d,\s]+)\]", title or "")
            return [int(x) for x in m.group(1).split(",")] if m else []

        async def read_row(idx):
            return await js(f"""(() => {{
              const s = document.querySelectorAll('.crafting .cmq-slider')[{idx}];
              return {{ dom: s.querySelector('.cqs-track')?.getAttribute('aria-valuenow'),
                        val: s.querySelector('.cmq-val')?.textContent.trim() }};
            }})()""")

        async def drag_slider(idx, frac):
            """Echter Drag: Thumb der Zeile idx auf frac (0..1) ziehen.
            Startet am AKTUELLEN Thumb (value→Pixel), 5 Zwischenschritte
            (natives pointermove-Verhalten), raw-Wert during drag lesen."""
            geo = await js(
                "(function(){var s=document.querySelectorAll('.crafting .cmq-slider')[%d];"
                "var tr=s.querySelector('.cqs-track');tr.scrollIntoView({block:'center'});"
                "var r=tr.getBoundingClientRect();"
                "return {x:r.left,y:r.top+r.height/2,w:r.width,v:Number(tr.getAttribute('aria-valuenow'))};})()" % idx)
            assert geo and geo["w"] > 0, f"no slider geometry: {geo}"
            # M8-R3 Custom-Slider: Thumb-Mitte = v/1000 der Track-Breite.
            def x_for(v):
                t = max(0.0, min(1.0, v / 1000.0))
                return geo["x"] + t * geo["w"]
            await mouse(x_for(geo["v"]), geo["y"], "mousePressed")
            await asyncio.sleep(0.1)
            raw = None
            for i in range(1, 6):
                f = geo["v"] + (frac * 1000 - geo["v"]) * i / 5
                await mouse(x_for(f), geo["y"], "mouseMoved")
                await asyncio.sleep(0.05)
                raw = await js(
                    "document.querySelectorAll('.crafting .cqs-track')[%d].getAttribute('aria-valuenow')" % idx)
            await mouse(x_for(frac * 1000), geo["y"], "mouseReleased")
            await asyncio.sleep(0.3)
            return raw

        async def wrap(expr):
            return ok(await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB + "\n" + expr}))

        await wrap(WRAP)
        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("openCraft('Quality Cannon')")
        await wait_for(
            "craftDetailOpen() && document.querySelectorAll('.crafting .cmq-slider').length >= 1", 8)

        rows = await js("sliderRows()")
        if rows and rows[0]["snapOn"] is False:
            await js("toggleSnapAt(0)")
            await asyncio.sleep(0.2)
        bands = await row_bands(0)
        check("D0 Bands aus Snap-Titel gelesen (>=2 Punkte)", len(bands) >= 2, str(bands))

        # ---- Drag 1: auf 60 % → raw ≈ 600 → nearest(Bands) ----
        raw = await drag_slider(0, 0.60)
        r = await read_row(0)
        expect = nearest(bands, int(raw or 600))
        print(f"  drag1: raw={raw} expect={expect} dom={r['dom']} state={r['val']}")
        check("D1 State = nearest(Bands) nach Release",
              r["val"] == str(expect), f"val={r['val']} expect={expect}")
        check("D2 Thumb/DOM-Wert ebenfalls auf dem Punkt (User-Sicht)",
              r["dom"] == str(expect), f"DOM={r['dom']} expect={expect}")

        # ---- Drag 2: NOCHMAL in denselben Bereich (kein garantierter
        #      State-Change) — Thumb muss trotzdem rasten ----
        raw2 = await drag_slider(0, 0.65)
        r = await read_row(0)
        expect2 = nearest(bands, int(raw2 or expect))
        print(f"  drag2: raw={raw2} expect={expect2} dom={r['dom']} state={r['val']}")
        check("D3 Zweiter Drag: Thumb rastet ebenfalls",
              r["dom"] == str(expect2) and r["val"] == str(expect2),
              f"DOM={r['dom']} val={r['val']} expect={expect2}")

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
