#!/usr/bin/env python3
"""t_dc152764 — Location-Detail: Gruppen-Header + Trenner je Shop/Terminal.

Stubbasiert (smoke-stub.js), wie die übrigen Smoke-Probes. Für Levski wird
eine Stub-Variante erzeugt (String-Patch: Location-Pool-Eintrag + eigener
get_prices-Zweig für id=130).

Vorher/Nachher: der Probe wird mit BUILD_DIR auf einen beliebigen Build
gezeigt (main = vorher, feature/location-group-headers = nachher).
"""
import asyncio, base64, json, os, pathlib, subprocess, time, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", "/home/sebo/Projects/Architects Overlay"))
BUILD = pathlib.Path(os.environ.get("BUILD_DIR", ROOT / "build"))
OUT = pathlib.Path(os.environ.get("OUT_DIR", ROOT / "docs" / "screens"))
OUT.mkdir(parents=True, exist_ok=True)
TAG = os.environ.get("TAG", "after")
PORT = int(os.environ.get("PORT", "8796"))

STUB_BASE = (ROOT / "docs/scripts/smoke-stub.js").read_text()

LEVSKI_GROUPS = """[
					{ terminal_id: 901, terminal_name: 'Garage - Levski', prices: [] },
					{
						terminal_id: 902, terminal_name: 'Commodity Shop - Levski',
						prices: [
							price('Levski — Commodity Shop', 902, 1108, 0, 0, { code: 'AGRS', commodity_id: 3 }, 'Agricultural Supplies'),
							price('Levski — Commodity Shop', 902, 0, 8300, 0, { code: 'TITA', commodity_id: 75 }, 'Titanium'),
							price('Levski — Commodity Shop', 902, 0, 12000, 0, { code: 'CMAT', commodity_id: 19 }, 'Construction Materials')
						]
					},
					{
						terminal_id: 903, terminal_name: 'Trade and Goods - Levski',
						prices: [
							price('Levski — Trade and Goods', 903, 727, 0, 0, { code: 'HELI', commodity_id: 38 }, 'Helium'),
							price('Levski — Trade and Goods', 903, 999, 0, 0, { code: 'LAMA', commodity_id: 88 }, 'Lama Feed'),
							price('Levski — Trade and Goods', 903, 0, 5400, 0, { code: 'LAMX', commodity_id: 89 }, 'Lama Extract'),
							price('Levski — Trade and Goods', 903, 0, 27000, 0, { code: 'BORA', commodity_id: 15 }, 'Borase')
						]
					},
					{
						terminal_id: 904, terminal_name: 'Medical Shop - Levski',
						prices: [
							price('Levski — Medical Shop', 904, 295, 0, 0, { code: 'MEDH', commodity_id: 77 }, 'MedPen (Hemozal)')
						]
					}
				]"""


def stub_for(loc):
    if loc == "checkmate":
        return STUB_BASE
    s = STUB_BASE
    # 1) Location-Pool: Levski-Eintrag ergänzen
    anchor = "location: [\n\t\t\t\t\t\t{ id: 436, name: 'Checkmate Station'"
    assert anchor in s, "location-pool anchor missing"
    s = s.replace(anchor,
        "location: [\n\t\t\t\t\t\t{ id: 130, name: 'Levski', kind: 'location', subtitle: 'Stanton > Hurston > Levski', code: null },\n\t\t\t\t\t\t{ id: 436, name: 'Checkmate Station'")
    # 2) get_prices: eigener Zweig für Levski (id=130) vor dem Checkmate-Block
    anchor2 = "if (a.kind === 'location') {"
    assert anchor2 in s, "location branch anchor missing"
    levski_branch = (
        "if (a.kind === 'location') {\n"
        "\t\t\t\t\tif (Number(a.id || 436) === 130) {\n"
        "\t\t\t\t\t\tconst groups = " + LEVSKI_GROUPS + ";\n"
        "\t\t\t\t\t\tconst prices = [].concat(...groups.map((g) => g.prices));\n"
        "\t\t\t\t\t\treturn { commodity: name, source: 'uex', fetched_at: now, prices,\n"
        "\t\t\t\t\t\t\tlocation: { location: 'Stanton > Hurston > Levski', terminal: 'Levski', groups } };\n"
        "\t\t\t\t\t}\n"
    )
    s = s.replace(anchor2, levski_branch, 1)
    return s


async def run_case(cdp, call, js, shot, loc, query, fname, title):
    stub = stub_for(loc)
    # Stub neu injizieren: über Page.addScriptToEvaluateOnNewDocument nur beim
    # ersten Load möglich → pro Case einen frischen Tab navigieren und Skript
    # via Runtime.evaluate VOR App-Boot setzen: einfacher Weg = navigate auf
    # about:blank, Script registrieren, dann navigieren.
    await call("Page.navigate", {"url": "about:blank"})
    await asyncio.sleep(0.3)
    await call("Page.addScriptToEvaluateOnNewDocument", {"source": stub})
    await call("Page.navigate", {"url": "http://127.0.0.1:%d/" % PORT})
    await asyncio.sleep(2.5)
    await js("(function(){var orig=window.__TAURI_INTERNALS__.invoke;window.__calls=[];window.__TAURI_INTERNALS__.invoke=function(c,a){window.__calls.push(c+':'+JSON.stringify(a||{}));return orig(c,a);};})();")
    async def type_query(text):
        await js("(function(){var i=document.querySelector('input.q');"
                 "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                 "set.call(i,'');var e=new InputEvent('input',{bubbles:true,inputType:'insertText',data:''});i.dispatchEvent(e);"
                 "set.call(i," + json.dumps(text) + ");"
                 "var e2=new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "});i.dispatchEvent(e2);"
                 "return i.value;})()")
        await asyncio.sleep(0.7)
    await type_query(query)
    await type_query(query)
    await asyncio.sleep(1.5)
    n_loc = await js(f"[...document.querySelectorAll('.row .name')].filter(n=>n.textContent.trim()==={json.dumps(title)}).length")
    print(f"  [{loc}] location rows:", n_loc)
    await js(f"var n=[...document.querySelectorAll('.row .name')].find(n=>n.textContent.trim()==={json.dumps(title)}); if(n) n.closest('.row').click();")
    await asyncio.sleep(1.5)
    info = await js("""(() => {
      const tabs=[...document.querySelectorAll('.d-body > .ptab')];
      const caps=tabs.map(t=>t.querySelector(':scope > .cap')?.textContent.trim());
      const borders=tabs.map(t=>getComputedStyle(t).borderTopWidth);
      const margins=tabs.map(t=>getComputedStyle(t).marginTop);
      return JSON.stringify({groups:tabs.length, caps, borders, margins,
        title:document.querySelector('.d-title h2')?.textContent||''});
    })()""")
    print(f"  [{loc}] detail:", info)
    await shot(f"{TAG}-{fname}-detail-top.png")
    # Scroll in die unteren Gruppen
    await js("var b=document.querySelector('.pane-detail'); if(b) b.scrollTop=b.scrollHeight;")
    await asyncio.sleep(0.5)
    await shot(f"{TAG}-{fname}-detail-scrolled.png")
    # Esc-Logik unberührt: Esc schließt Detail zurück zur Liste
    await js("document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))")
    await asyncio.sleep(0.5)
    print(f"  [{loc}] after-esc pane-detail:", await js("document.querySelectorAll('.pane-detail').length"),
          "rows:", await js("document.querySelectorAll('.row').length"))


async def main():
    subprocess.run(["pkill", "-f", f"http.server {PORT} "], stderr=subprocess.DEVNULL)
    subprocess.run(["pkill", "-f", f"http.server {PORT}$"], stderr=subprocess.DEVNULL)
    srv = subprocess.Popen(["python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
                            "--directory", str(BUILD)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(40):
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PORT}/", timeout=1); break
        except Exception: time.sleep(0.25)
    subprocess.run(["pkill", "-f", "remote-debugging-port=9227"], stderr=subprocess.DEVNULL)
    chr = subprocess.Popen(["/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
                            "--window-size=1920,1080", "--hide-scrollbars",
                            "--remote-debugging-port=9227", "--user-data-dir=/tmp/cdp-locgroup",
                            "about:blank"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ws = None
    for _ in range(60):
        try:
            pages = [t for t in json.load(urllib.request.urlopen("http://127.0.0.1:9227/json/list", timeout=1)) if t.get("type") == "page"]
            if pages: ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception: time.sleep(0.25)
    if not ws:
        srv.terminate(); chr.terminate(); raise SystemExit("no CDP target")
    try:
        async with websockets.connect(ws, max_size=None) as cdp:
            mid = {"n": 0}; pend = {}
            async def call(m, p=None):
                mid["n"] += 1; i = mid["n"]; fut = asyncio.get_event_loop().create_future(); pend[i] = fut
                await cdp.send(json.dumps({"id": i, "method": m, "params": p or {}}))
                return await fut
            async def recv():
                async for raw in cdp:
                    msg = json.loads(raw)
                    if "id" in msg and msg["id"] in pend: pend[msg["id"]].set_result(msg)
            asyncio.get_event_loop().create_task(recv())
            def ok(r):
                if "error" in r: raise RuntimeError("CDP error: %s" % r["error"])
                return r.get("result", {})
            async def js(expr):
                r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"): return "EXC:" + r["exceptionDetails"].get("text", "")
                return r.get("result", {}).get("value")
            async def shot(name):
                r = ok(await call("Page.captureScreenshot", {"format": "png"}))
                p = OUT / name; p.write_bytes(base64.b64decode(r["data"]))
                print("  shot:", p, len(r["data"]), "bytes")
            await call("Page.enable"); await call("Runtime.enable")
            await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
            await run_case(cdp, call, js, shot, "levski", "levski", "levski", "Levski")
            await run_case(cdp, call, js, shot, "checkmate", "checkmate", "checkmate", "Checkmate Station")
    finally:
        chr.terminate(); srv.terminate()

asyncio.run(main())