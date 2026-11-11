#!/usr/bin/env python3
"""CDP-Probe M6-F3 (t_7e4a92c9) — Deckkraft: globaler Standard + editierbare Presets.

Funktionale DOM-Assertions auf feature/m6-f3-opacity:
  A — Settings: Abschnitt "Pins" mit Slider + Zahl-Eingabe für den globalen
      Standard-Deckkraft-Wert und 3 editierbaren Presets.
  B — IPC: Zahl-Änderung → invoke('settings_set_pins_default_opacity'),
      Preset-Änderung → invoke('settings_set_pins_presets') mit Snapshot-Update.
  C — PinBoxes: Presets kommen aus pins_state (stubbbar, nicht hart codiert),
      Preset-Buttons zeigen die gestubbten Werte.
  D — Preset-Zyklus: cycleOpacity nimmt den nächsten Wert ÜBER dem aktuellen
      (Wrap zum kleinsten).

Backend-Seite (Clamp/Normierung, Persistenz, pin_add erbt default_opacity)
durch cargo tests (settings-Crate + pins.rs) belegt.

Self-contained: startet statischen Server (build/) + headless Chromium.
"""
import asyncio, base64, json, os, pathlib
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8813"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9241"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

SNAPSHOT = {
    "palette": "amber", "detail_variant": "c", "scale": 100,
    "click_through": False, "spotlight_monitor": 0,
    "spotlight_enabled": False, "always_on_top": True, "auto_launch": False,
    "check_updates": True, "updater_enabled": True, "updater_interval_hours": 24,
    "language": "en", "pins_default_opacity": 55, "pins_presets": [20, 55, 100],
}

def stub_extra(pins_js="[]"):
    """settings_get/set_* + pins_state über __m6f3-Override (pins_js: Pin-JSON)."""
    return """
(function() {
  window.__m6f3 = { snap: %s,
    pins: { enabled: true, game_running: true, overlay_visible: true,
      spotlight_screen: 'DP-3', default_opacity: 55, presets: [30, 60, 90],
      pins: %s } };
  const orig = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    globalThis.__lastInvoke = { cmd, args: args || {} };
    (globalThis.__invokes = globalThis.__invokes || []).push({ cmd, args: args || {} });
    if (cmd === 'overlay_get_state') return { visible: true, game_running: true, click_through: false };
    if (cmd === 'settings_get') return window.__m6f3.snap;
    if (cmd === 'pins_state') return window.__m6f3.pins;
    if (cmd === 'settings_set_pins_default_opacity') {
      window.__m6f3.snap = Object.assign({}, window.__m6f3.snap,
        { pins_default_opacity: args.opacity });
      return window.__m6f3.snap;
    }
    if (cmd === 'settings_set_pins_presets') {
      window.__m6f3.snap = Object.assign({}, window.__m6f3.snap,
        { pins_presets: args.presets });
      return window.__m6f3.snap;
    }
    return orig(cmd, args);
  };
})();
""" % (json.dumps(SNAPSHOT), pins_js)

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
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-m6f3-probe",
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

        # =================================================================
        # A+B — Settings: Pins-Abschnitt, Slider + Zahl, IPC auf set-Commands
        # =================================================================
        await navigate(stub_extra())
        # Settings über die Rail öffnen (ao-window-open-config feuert nur im
        # Fenstermodus, nicht im Spotlight-Boot): Settings-Rail-Entry klicken.
        await js("""(function(){
            var btns = Array.from(document.querySelectorAll('.rail button'));
            var b = btns.find(function(x){ return /settings/i.test(x.title || ''); })
                 || btns[btns.length - 1];
            if (!b) return 'NO_BTN';
            b.click(); return 'clicked:' + (b.title || '');
        })()""")
        await asyncio.sleep(1.5)
        sects = await js("""(function(){
            var secs = Array.from(document.querySelectorAll('section.cfg-sect'));
            var s = secs.find(function(x){ return (x.querySelector('div.h')||{}).textContent === 'Pins'; });
            if (!s) return 'NONE';
            return JSON.stringify({
              sliders: s.querySelectorAll('input[type=range]').length,
              numbers: s.querySelectorAll('input[type=number]').length,
              notes: s.querySelectorAll('.cfg-note').length,
              slider_val: (s.querySelector('input[type=range]')||{}).value,
              num_vals: Array.from(s.querySelectorAll('input[type=number]')).map(function(i){return i.value;}),
              slider_min: (s.querySelector('input[type=range]')||{}).min
            });
        })()""")
        try:
            a = json.loads(str(sects))
        except Exception:
            a = {}
        check("a-settings-pins-section-present", bool(a), str(sects)[:120])
        check("a-slider-plus-number-inputs", a.get("sliders") == 1 and a.get("numbers") == 4,
              f"sliders={a.get('sliders')} numbers={a.get('numbers')}")
        check("a-slider-bounds-20-100-default-55", str(a.get("slider_min")) == "20" and str(a.get("slider_val")) == "55",
              f"min={a.get('slider_min')} val={a.get('slider_val')}")
        check("a-preset-numbers-3", a.get("num_vals")[-3:] == ["20", "55", "100"], str(a.get("num_vals")))

        # Zahl-Eingabe für Default → settings_set_pins_default_opacity
        await js("""(function(){
            var secs = Array.from(document.querySelectorAll('section.cfg-sect'));
            var s = secs.find(function(x){ return (x.querySelector('div.h')||{}).textContent === 'Pins'; });
            var num = s.querySelector('input[type=number]');
            num.value = '78'; num.dispatchEvent(new Event('input', {bubbles:true}));
            num.dispatchEvent(new Event('change', {bubbles:true}));
            return true;
        })()""")
        await asyncio.sleep(0.5)
        inv = await js("(function(){var iv=(globalThis.__invokes||[]).filter(function(x){return x.cmd==='settings_set_pins_default_opacity';}); return iv.length ? JSON.stringify(iv[iv.length-1].args) : 'NONE';})()")
        check("b-default-number-invokes-set", '"opacity":78' in str(inv), str(inv))
        snap_after = await js("(function(){return document.querySelector('section.cfg-sect') ? 'ok' : 'err';})()")
        cfg_val = await js("""(function(){
            var secs = Array.from(document.querySelectorAll('section.cfg-sect'));
            var s = secs.find(function(x){ return (x.querySelector('div.h')||{}).textContent === 'Pins'; });
            return JSON.stringify({
              slider: s.querySelector('input[type=range]').value,
              num: s.querySelector('input[type=number]').value });
        })()""")
        check("b-ui-updated-to-78", '"78"' in str(cfg_val), str(cfg_val))

        # Preset #2 ändern → settings_set_pins_presets mit Array
        await js("""(function(){
            var secs = Array.from(document.querySelectorAll('section.cfg-sect'));
            var s = secs.find(function(x){ return (x.querySelector('div.h')||{}).textContent === 'Pins'; });
            var nums = s.querySelectorAll('input[type=number]');
            var p = nums[nums.length - 2]; // Preset #2 (letzte 3 sind die Presets)
            p.value = '70'; p.dispatchEvent(new Event('input', {bubbles:true}));
            p.dispatchEvent(new Event('change', {bubbles:true}));
            return true;
        })()""")
        await asyncio.sleep(0.5)
        inv2 = await js("(function(){var iv=(globalThis.__invokes||[]).filter(function(x){return x.cmd==='settings_set_pins_presets';}); return iv.length ? JSON.stringify(iv[iv.length-1].args) : 'NONE';})()")
        check("b-preset-edit-invokes-set-array", str(inv2).count("70") >= 1 and "presets" in str(inv2), str(inv2))
        await js("""(function(){
            var secs = Array.from(document.querySelectorAll('section.cfg-sect'));
            var s = secs.find(function(x){ return (x.querySelector('div.h')||{}).textContent === 'Pins'; });
            if (s) s.scrollIntoView({block: 'center'});
            return 'ok';
        })()""")
        await asyncio.sleep(0.4)
        await shot("m6f3-a-settings-pins-section.png")

        # =================================================================
        # C+D — PinBoxes: Presets aus pins_state, Zyklus oben-wrapped
        # =================================================================
        pin = {"view_id": "routes", "screen": "DP-3", "box_index": 0, "opacity": 45,
               "pinned": True, "x": 400, "y": 200, "title": "Route 1", "kpi": "k",
               "rows": [{"idx": "01", "name": "S", "sub": "C", "value": "1", "side": "buy"}]}
        await navigate(stub_extra(pins_js=json.dumps([pin])))  # Reload mit Pins + Presets 30/60/90
        btns = await js("""(function(){
            var head = document.querySelector('main .pin .pin-head');
            head.dispatchEvent(new PointerEvent('pointerdown', {bubbles:true, pointerId: 2}));
            window.dispatchEvent(new PointerEvent('pointerup', {bubbles:true, pointerId: 2, movementX: 0, movementY: 0}));
            return true;
        })()""")
        await asyncio.sleep(0.6)
        labels = await js("""(function(){
            var pop = document.querySelector('.pin-popover');
            if (!pop) return 'NO_POPOVER';
            return JSON.stringify(Array.from(pop.querySelectorAll('.pbtn')).map(function(b){return b.textContent.trim();}));
        })()""")
        try:
            labs = json.loads(str(labels))
        except Exception:
            labs = []
        preset_btns = [l for l in labs if l.replace("−", "-").replace("+", "").replace("◐", "").replace("✕", "").strip().isdigit()]
        check("c-preset-buttons-from-settings", preset_btns == ["30", "60", "90"],
              f"buttons={labs} presets={preset_btns}")
        # D — Zyklus: Pin bei 45 → nächster Wert über 45 ist 60 (nicht 30)
        await js("""(function(){
            var pop = document.querySelector('.pin-popover');
            var cyc = Array.from(pop.querySelectorAll('.pbtn')).find(function(b){return b.textContent.trim()==='◐';});
            cyc.click(); return true;
        })()""")
        await asyncio.sleep(0.5)
        op = await js("(function(){var iv=(globalThis.__invokes||[]).filter(function(x){return x.cmd==='pin_set_opacity';}); return iv.length ? JSON.stringify(iv[iv.length-1].args) : 'NONE';})()")
        check("d-cycle-next-above-current", '"opacity":60' in str(op), str(op))
        await shot("m6f3-c-pin-popover-editable-presets.png")

    print(f"\nRESULT: PASS={len(PASS)} FAIL={len(FAIL)}")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
    proc.kill(); server.kill()
    return 1 if FAIL else 0

if __name__ == "__main__":
    import sys
    sys.exit(asyncio.run(main()))
