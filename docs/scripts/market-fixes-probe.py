#!/usr/bin/env python3
"""CDP-Probe für die Market-Fixes (t_3402ae7e) auf feature/market-fixes.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  P1 — Filter-Root-Cause: WTS-Term+Sortierung und alle-Modus-Term+Sortierung
       liefern gefilterte, sortierte Listen (der Provider-Fetch-Plan ist
       cargo-test-belegt; dieser Probe prüft die Svelte-Logik gegen den
       Mirror-Kontrakt in smoke-stub.js).
  P2 — Systemfilter raus: keine Region-Chips/-Label mehr im Market-Modus.
  P5 — Input-Reset: Market→Search (Tab und M) leert das Eingabefeld und
       setzt Sortierung/Operation/Qualität auf die Defaults zurück.

Datenquelle: smoke-stub.js (JS-Mirror des Provider-Kontrakts) — die Rust-Seite
ist durch cargo tests belegt (crates/provider apply_marketplace_filters,
crates/uex-client Fetch-Plan). Screenshots nach docs/screens/.

Self-contained: startet den statischen Server (build/) und headless Chromium.
"""
import asyncio, base64, json, os, pathlib, sys
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
URL = os.environ.get("PROBE_URL", "http://127.0.0.1:8795/")

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

async def main():
    # ---- static server (self-contained, lives for this run) ----
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8795", "--bind", "127.0.0.1",
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
        raise RuntimeError("static server on 8795 did not come up")
    print("static server up on 8795")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9227", "--user-data-dir=/tmp/cdp-market-fixes",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9227/json/list", timeout=1) as r:
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

        VK = {"m": (77, "KeyM"), "Tab": (9, "Tab")}
        async def key(k, ctrl=False):
            vk, code = VK[k]
            mods = 2 if ctrl else 0
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
                                                  "modifiers": mods})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
                                                  "modifiers": mods})
            await asyncio.sleep(0.2)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = base64.b64decode(r["data"])
            (OUT / name).write_bytes(data)
            print("  shot:", OUT / name, len(data), "bytes")

        def set_input_expr(sel, text):
            return (
                "(function(){"
                "var i=document.querySelector(" + json.dumps(sel) + ");"
                "if(!i) return 'NO-ELEMENT';"
                "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                "set.call(i,'');"
                "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                "set.call(i," + json.dumps(text) + ");"
                "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                "return i.value;"
                "})()"
            )

        async def typeQuery(text, sel="input.q"):
            await js("var i=document.querySelector('input.q'); if(i) i.focus();")
            await js(set_input_expr(sel, text))
            await asyncio.sleep(0.7)  # Debounce 180 ms + invoke

        async def click_chip(label):
            return await js(
                "(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()"
            )

        async def market_rows():
            return json.loads(str(await js("""(function(){
                return JSON.stringify([...document.querySelectorAll('.mrow')].map(function(r){
                    var op=r.querySelector('.m-op'); var t=r.querySelector('.m-title'); var pr=r.querySelector('.m-price');
                    return { op: op ? op.textContent.trim() : '',
                             title: t ? t.textContent.trim() : '',
                             priceText: pr ? pr.textContent.trim() : '' };
                }));
            })()""") or "[]"))

        def price_num(price_text):
            # fmtPrice uses de-DE grouping ("1.234.567") — strip dots/spaces
            return float(price_text.replace(".", "").replace(",", ".").replace(" ", ""))

        # invoke-Log instrumentieren
        await js("""(function(){
            var orig = window.__TAURI_INTERNALS__.invoke;
            window.__calls = [];
            window.__TAURI_INTERNALS__.invoke = function(cmd,args){
                window.__calls.push(cmd + ':' + JSON.stringify(args || {}));
                return orig(cmd,args);
            };
        })()""")

        # ---- 0. Warm-up + Market-Modus betreten ------------------------------
        await typeQuery("WARM")
        await typeQuery("")
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        await key("m", ctrl=True)
        await asyncio.sleep(1.2)
        smode = await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")
        check("market-entered", smode.strip() == "MARKETPLACE", smode)

        # ---- P2: Systemfilter (Region) entfernt ------------------------------
        mbar_text = await js("[...document.querySelectorAll('.mbar, .mbar2')].map(function(b){return b.textContent.replace(/\\s+/g,' ').trim()}).join(' | ')")
        check("p2-no-region-label", "Region" not in (mbar_text or ""), mbar_text[:160])
        region_chips = await js("[...document.querySelectorAll('.mbar .chip, .mbar2 .chip')].filter(function(c){return ['Stanton','Nyx','Pyro'].indexOf(c.textContent.trim())>=0}).length")
        check("p2-no-region-chips", region_chips == 0, "region-chips=" + str(region_chips))
        await shot("15-market-no-region-filter.png")

        # ---- P1a: WTS 'Parallax' Preis ↓ → sortierte Liste --------------------
        clicked = await click_chip("Verkauf")
        check("wts-chip-clicked", clicked is True)
        await asyncio.sleep(1.0)
        await typeQuery("Parallax")
        await asyncio.sleep(0.6)
        await click_chip("Preis ↓")
        await asyncio.sleep(1.0)
        rows = await market_rows()
        check("p1a-wts-parallax-rows", len(rows) >= 15, "rows=" + str(len(rows)))
        check("p1a-all-wts", all(r["op"] == "WTS" for r in rows), str([r["op"] for r in rows[:5]]))
        check("p1a-all-parallax", all("parallax" in r["title"].lower() for r in rows),
              str([r["title"] for r in rows[:3]]))
        price_vals = [price_num(r["priceText"]) for r in rows]
        descending = all(price_vals[i] >= price_vals[i + 1] for i in range(len(price_vals) - 1))
        check("p1a-price-desc-sorted", descending and len(price_vals) >= 2,
              "first=" + str(price_vals[:3]) + " last=" + str(price_vals[-2:]))
        await shot("16-wts-parallax-price-desc.png")

        # ---- P1b: alle-Modus mit Term + Sortierung ----------------------------
        clicked = await click_chip("Alle")
        check("all-chip-clicked", clicked is True)
        await asyncio.sleep(1.0)
        await typeQuery("gold")
        await asyncio.sleep(0.6)
        await click_chip("Preis ↑")
        await asyncio.sleep(1.0)
        rows = await market_rows()
        check("p1b-all-gold-rows", len(rows) >= 2, "rows=" + str(len(rows)))
        check("p1b-all-gold-filtered", all("gold" in r["title"].lower() for r in rows),
              str([r["title"] for r in rows[:4]]))
        price_vals = [price_num(r["priceText"]) for r in rows]
        ascending = all(price_vals[i] <= price_vals[i + 1] for i in range(len(price_vals) - 1))
        check("p1b-price-asc-sorted", ascending and len(price_vals) >= 2, str(price_vals))
        last_sort = await js("(globalThis.__lastMarket || {}).filters ? globalThis.__lastMarket.filters.sort : null")
        last_op = await js("(globalThis.__lastMarket || {}).filters ? globalThis.__lastMarket.filters.operation : null")
        check("p1b-backend-got-term-sort-op", last_sort == "price_asc" and last_op is None,
              "sort=" + str(last_sort) + " op=" + str(last_op))
        await shot("17-all-gold-price-asc.png")

        # ---- P5: Market → Search (Tab) leert Eingabefeld + Filter -------------
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        await key("Tab")
        await asyncio.sleep(0.6)
        smode = await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")
        val = await js("document.querySelector('input.q') ? document.querySelector('input.q').value : 'NO-FIELD'")
        check("p5-tab-back-to-search", smode.strip() == "SUCHE", smode)
        check("p5-tab-field-empty", val == "", "value=" + json.dumps(val))

        # ---- P5: Market → Search (M) nach neuer Filter-Nutzung ----------------
        await key("m", ctrl=True)
        await asyncio.sleep(1.2)
        await click_chip("Verkauf")
        await asyncio.sleep(0.8)
        await click_chip("Preis ↓")
        await asyncio.sleep(0.8)
        await typeQuery("parallax")
        await asyncio.sleep(0.4)
        await js(set_input_expr("input.qnum", "500"))
        await asyncio.sleep(0.4)
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        await key("m", ctrl=True)
        await asyncio.sleep(0.6)
        smode = await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")
        val = await js("document.querySelector('input.q') ? document.querySelector('input.q').value : 'NO-FIELD'")
        check("p5-m-back-to-search", smode.strip() == "SUCHE", smode)
        check("p5-m-field-empty", val == "", "value=" + json.dumps(val))

        # Wieder rein: Market startet unbelastet (Defaults, volle Liste) —
        # die Zurücksetzung der Markt-Zustände ist erst hier im DOM prüfbar.
        await key("m", ctrl=True)
        await asyncio.sleep(1.2)
        all_on = await js("(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()==='Alle'}); return b ? b.classList.contains('is-on') : 'no-chip';})()")
        newest_on = await js("(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()==='Neueste'}); return b ? b.classList.contains('is-on') : 'no-chip';})()")
        qmin = await js("document.querySelector('.qnum') ? document.querySelector('.qnum').value : 'NONE'")
        check("p5-op-reset-to-all", all_on is True, str(all_on))
        check("p5-sort-reset-to-newest", newest_on is True, str(newest_on))
        check("p5-quality-reset", qmin == "", "qmin=" + str(qmin))
        rows = await market_rows()
        check("p5-reenter-fresh-list", len(rows) >= 15, "rows=" + str(len(rows)))
        last_m = await js("JSON.stringify(globalThis.__lastMarket && globalThis.__lastMarket.filters || {})")
        expected = json.dumps({"location": "", "q": "", "item_id": None, "operation": None, "region": "", "sort": "newest", "limit": 100}, separators=(",", ":"))
        check("p5-reenter-default-filters", last_m == expected, last_m)
        await shot("18-market-reenter-fresh.png")

        console_events = console_events[:8]
        check("no-console-exceptions", len(console_events) == 0, str(console_events))

    print("\n===== RESULTAT =====")
    print("PASS:", len(PASS), " FAIL:", len(FAIL))
    for f in FAIL:
        print("  FAILED:", f)
    proc.terminate()
    server.terminate()
    return 1 if FAIL else 0

if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
