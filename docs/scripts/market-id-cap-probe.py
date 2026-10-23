#!/usr/bin/env python3
"""CDP-Probe für t_790ef376 (id-Resolution auf ALLE Begriffs-Treffer).

Belegt das ≥15-Offers-Kriterium auf der echten Svelte-Oberfläche:
  1. Market 'parallax' → 112 Offers (echter Payload aus live_dump_market_offers,
     echte UEX-API über den echten App-Cache) — erste Seite voll (15) + Load-More.
  2. Min-Q 900 gesetzt → visOffers = 36 ≥ 15, Liste bleibt voll + Load-More
     (vor dem Fix: 5Offers → Liste fällt unter eine Seite).
  3. Sortierung 'Preis ↑' → sichtbare Preise aufsteigend.

Die Rust-Seite (Fetch-Plan, alle id_item-Fetches) ist cargo-test-belegt;
der Qualitätsfilter ist UI-seitig (visOffers/qualityPasses) — genau diese
echte Frontend-Logik läuft hier über die echten Offer-Daten.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/market-id-cap")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8795/"
REAL = json.load(open(os.environ["REAL_OFFERS"]))

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8795", "--bind", "127.0.0.1",
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
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9229", "--user-data-dir=/tmp/cdp-market-id-cap",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9229/json/list", timeout=1) as r:
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
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        VK = {"m": (77, "KeyM")}
        async def key(k, ctrl=False):
            vk, code = VK[k]
            mods = 2 if ctrl else 0
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk, "modifiers": mods})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk, "modifiers": mods})
            await asyncio.sleep(0.2)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", OUT / name)

        def set_input_expr(sel, text):
            return ("(function(){var i=document.querySelector(" + json.dumps(sel) + ");"
                    "if(!i) return 'NO-ELEMENT';"
                    "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                    "set.call(i,'');"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                    "set.call(i," + json.dumps(text) + ");"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                    "return i.value;})()")

        # Echter Offer-Payload (live_dump_market_offers) in die IPC-Schicht
        # injizieren — nur die invoke-Grenze ist ersetzt, Filter/Sort/Liste
        # laufen in der echten Svelte-Komponente.
        payload = json.dumps({"source": "cache", "fetched_at": 0, "offers": REAL, "note": None})
        await js("window.__TAURI_INTERNALS__.origInvoke = window.__TAURI_INTERNALS__.invoke;"
                 "window.__TAURI_INTERNALS__.invoke = function(cmd,args){"
                 "if(cmd==='get_marketplace'){return Promise.resolve(" + payload + ");}"
                 "return window.__TAURI_INTERNALS__.origInvoke(cmd,args);};")

        # Market-Modus betreten, echte Query
        await js("var i=document.querySelector('input.q'); if(i) i.focus();")
        await key("m", ctrl=True)
        await asyncio.sleep(1.0)
        smode = await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")
        check("market-entered", str(smode).strip() == "MARKETPLACE", str(smode))
        await js(set_input_expr("input.q", "parallax"))
        await asyncio.sleep(1.2)

        async def rows():
            return json.loads(await js("JSON.stringify([...document.querySelectorAll('.mrow')].length)"))
        n_all = int(await rows())
        check("unfiltered-parallax-112", n_all == 15, "rows on page=" + str(n_all) + " (Erwartung: volle Seite 15 von 112)")
        total_note = await js("(function(){var n=document.body.innerText.match(/(\\d+) Offers/); return n?n[1]:'';})()")
        print("  offers-total-label:", total_note)
        load_more = await js("!!document.querySelector('.btn-more')")
        check("load-more-present", bool(load_more))
        await shot("01-market-parallax-all.png")

        # Sortierung Preis ↑
        await js("[...document.querySelectorAll('.mbar2 .chip')].find(function(c){return c.textContent.trim()==='Preis ↑'}).click()")
        await asyncio.sleep(0.4)

        # Min-Q 900 — ECHTER UI-Filter (visOffers/qualityPasses)
        await js(set_input_expr(".mbar2 .qnum", "900"))
        await asyncio.sleep(0.6)
        n_q = int(await rows())
        check("minq900-page-still-full", n_q == 15, "rows=" + str(n_q) + " (vor Fix: 5 — Liste unter einer Seite)")
        load_more_q = await js("!!document.querySelector('.btn-more')")
        check("load-more-after-filter", bool(load_more_q))
        await js("document.querySelector('.btn-more').click()")
        await asyncio.sleep(0.4)
        n_q2 = int(await rows())
        check("load-more-paginates-q900", n_q2 == 30, "rows after load-more=" + str(n_q2))
        prices = json.loads(await js("JSON.stringify([...document.querySelectorAll('.mrow .m-price')].map(function(p){return p.textContent.trim();}))"))
        def num(t):
            return float(t.replace(".", "").replace(",", ".").replace(" ", ""))
        nums = [num(p) for p in prices]
        check("price-asc-sorted", all(nums[i] <= nums[i+1] for i in range(len(nums)-1)), str(nums[:5]))
        await shot("02-market-parallax-minq900.png")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS, {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())
