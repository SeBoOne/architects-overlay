#!/usr/bin/env python3
"""v0.1.5 smoke probe — self-contained: static server + headless Chromium + CDP.

Gates:
  1. Detail-Seite Commodity 'Gold'  → 43 Preiszeilen (.pane-detail .pr)
  2. (obsolet — Location-Lock entfernt; Beleg: docs/screenshots/v0.1.5/README.md)
  3. Marketplace                       → 15 Angebote + "Weitere laden" → 30

Backend = Tauri-Stub (docs/scripts/smoke-stub.js), injected per-CDP.
"""
import asyncio, base64, json, os, pathlib, sys, urllib.request, urllib.error
import websockets, signal

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", "/home/sebo/Projects/Architects Overlay"))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/v0.1.5")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8795/"

PASS = 0
FAIL = 0


def check(name, cond, detail=""):
    global PASS, FAIL
    tag = "PASS" if cond else "FAIL"
    if cond:
        PASS += 1
    else:
        FAIL += 1
    print(f"  {tag} {name}" + (f"  {detail}" if not cond else ""))


async def main():
    # ---- static server (self-contained, lives for this run) ----
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
        raise RuntimeError("static server on 8795 did not come up")
    print("static server up on 8795")

    logs = []
    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9223", "--user-data-dir=/tmp/cdp-smoke3",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9223/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        server.terminate(); proc.terminate()
        raise RuntimeError("chromium CDP page target did not come up")

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
                elif msg.get("method") in ("Runtime.exceptionThrown", "Runtime.consoleAPICalled", "Log.entryAdded", "Runtime.uncaughtExceptionThrown"):
                    p = msg.get("params", {})
                    stack = (p.get("stackTrace", {}).get("callFrames", [])[:1]
                             if isinstance(p, dict) else [])
                    text = p.get("text") if isinstance(p, dict) else ""
                    if msg.get("method") == "Runtime.consoleAPICalled":
                        args = p.get("args", [])
                        text = (args[0].get("value") if args else "") if isinstance(args, list) else ""
                    logs.append(msg["method"] + ": " + str(text)[:200])

        asyncio.create_task(recv_loop())

        def ok(resp):
            if "error" in resp:
                raise RuntimeError("CDP error: %s" % resp["error"])
            return resp.get("result", {})

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {
                "expression": expr, "returnByValue": True, "awaitPromise": True,
            }))
            if r.get("exceptionDetails"):
                ed = r["exceptionDetails"]
                txt = ed.get("text", "") + " " + (ed.get("exception", {}).get("description", "") or "")
                logs.append("RUNTIME-EXC: " + str(txt)[:300])
                return None
            return r.get("result", {}).get("value")

        async def wait_for(expr, want=True, timeout=10):
            v = None
            end = asyncio.get_event_loop().time() + timeout
            while asyncio.get_event_loop().time() < end:
                v = await js(expr)
                if callable(want):
                    if v is not None and want(v):
                        return v
                elif want is True:
                    if v:
                        return v
                elif v == want:
                    return v
                await asyncio.sleep(0.15)
            return v

        async def typeQuery(text):
            await js("document.querySelector('input.q') && document.querySelector('input.q').focus()")
            expr = (
                "(function(){"
                "var i=document.querySelector('input.q');"
                "if(!i){return null;}"
                "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                "set.call(i,'');"
                "var ev=new InputEvent('input',{bubbles:true,inputType:'insertText',data:''});i.dispatchEvent(ev);"
                "set.call(i," + json.dumps(text) + ");"
                "var ev2=new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "});i.dispatchEvent(ev2);"
                "return i.value;})()"
            )
            await js(expr)
            await asyncio.sleep(0.7)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = base64.b64decode(r["data"])
            (OUT / (name + ".png")).write_bytes(data)
            print(f"  shot: {OUT / (name + '.png')}  ({len(data)} bytes)")

        async def key(k, **mods):
            opts = {"key": k, "bubbles": True, "composed": True}
            opts.update(mods)
            await js("window.dispatchEvent(new KeyboardEvent('keydown',%s))" % json.dumps(opts))
            await asyncio.sleep(0.3)

        async def esc():
            await key("Escape")

        async def txt(sel):
            return await js(
                "(function(){var e=document.querySelector('" + sel + "');return e?e.textContent.trim():''})()"
            )

        await call("Page.enable")
        await call("Runtime.enable")
        await call("Log.enable")
        await call("Emulation.setDeviceMetricsOverride",
                   {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        # instrument invoke logging
        await call("Runtime.evaluate", {
            "expression": """
            (function(){
              var ti = window.__TAURI_INTERNALS__;
              if(ti && ti.invoke && !ti.invoke.__logged){
                var orig = ti.invoke.bind(ti);
                ti.invoke = function(cmd,args){
                  window.__calls = window.__calls || [];
                  try { window.__calls.push(cmd + ': ' + JSON.stringify((args||{}).query||(args||{}).expr||args||{})); } catch(e){}
                  return orig(cmd,args);
                };
                ti.invoke.__logged = true;
              }
            })();
            """,
            "returnByValue": True,
        })
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(3.0)

        # ---- boot diagnostics ----
        body_len = await js("document.body ? document.body.innerHTML.length : 0")
        smode = await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : ''")
        idle = await js("document.querySelector('.pane-idle') ? 1 : 0")
        loading = await js("document.querySelector('.pane-loading') ? 1 : 0")
        err = await js("document.querySelector('.pane-error') ? 1 : 0")
        rows0 = await js("document.querySelectorAll('.row').length")
        dw = await js("document.documentElement.dataset.window")
        print(f"boot: body_html_len={body_len} smode={smode!r} panes idle/loading/error={idle}/{loading}/{err} .row={rows0} data-window={dw}")
        inv = await js("(async()=>{try{return await window.__TAURI_INTERNALS__.invoke('sync_status',{});}catch(e){return 'ERR:'+e.message;}})()")
        print(f"boot: direct invoke(sync_status)={json.dumps(inv)[:200] if inv else inv}")
        await asyncio.sleep(1.0)
        idle2 = await js("document.querySelector('.pane-idle') ? 1 : 0")
        loading2 = await js("document.querySelector('.pane-loading') ? 1 : 0")
        err2 = await js("document.querySelector('.pane-error') ? 1 : 0")
        cj = await js("JSON.stringify(window.__calls||[])")
        print(f"boot +1s: idle={idle2} loading={loading2} error={err2} calls={str(cj)[:300]}")

        # ===================================================================
        # 0) Item-Detail: kaubares Item 'CSP-68H Backpack' → echte Preise
        #    (Point 8: nicht mehr "nicht kaufbar", sondern /items_prices)
        # ===================================================================
        print("scenario 0: Item-Detail CSP-68H (Point 8)")
        await typeQuery("csp-68h")
        await typeQuery("csp-68h")
        await wait_for("document.querySelectorAll('.row').length", want=lambda x: x and x >= 1, timeout=8)
        irows = await js("document.querySelectorAll('.row').length")
        iname = await txt(".row .name")
        print(f"  rows={irows} first={iname!r}")
        await js("document.querySelector('.row').click()")
        await wait_for("document.querySelector('.pane-detail') ? 1 : 0", want=1, timeout=8)
        dpr = await js("document.querySelectorAll('.pane-detail .pr').length")
        ih2 = await txt(".d-head h2")
        print(f"  .pr={dpr} h2={ih2!r}")
        check("item: detail opens with prices (Point 8)", (dpr or 0) >= 1, f".pr={dpr}")
        check("item: title is CSP-68H Backpack", "CSP-68H Backpack" in (ih2 or ""), f"h2={ih2!r}")
        idetail_text = await js("document.querySelector('.pane-detail') ? document.querySelector('.pane-detail').innerText : ''") or ''
        check("item: not 'nicht kaufbar' (Point 8)", "nicht kaufbar" not in idetail_text, f"detail_text={idetail_text[:80]!r}")
        # QA Runde 2 (Punkt C): die API sendet `"price_sell": null` für Items
        # (shops kaufen nichts zurück). Der Stub sourct eine Null-Zeile und
        # mappt sie auf der SEAM-Ebene zu 0 — REGRESSION: ohne Fix reißt die
        # Null-Zeile die Tabelle (Json-Fehler). Hier: Zeile existiert, kein
        # "Fehler"-Paneel, Preis 0 (kein NaN/undefined).
        idpr0 = await js("document.querySelectorAll('.pane-detail .pr').length")
        check("item: null-sell row survives (Punkt C), no UI error", (idpr0 or 0) >= 1 and "Fehler" not in idetail_text, f".pr={idpr0} text={idetail_text[:60]!r}")
        jsell0 = await js("(document.querySelector('.pane-detail .pr .val.sell')?.innerText || '').trim()")
        check("item: null-sell maps to a numeric 0 / dash (Punkt C)", "NaN" not in (jsell0 or ""), f"sell={jsell0!r}")
        await shot("00-item-detail-csp68h")
        await esc()
        await wait_for("document.querySelector('.pane-detail') ? 0 : 1", want=1, timeout=5)

        # ===================================================================
        # 1) Detail-Seite: Commodity 'Gold' → 43 Preiszeilen
        # ===================================================================
        print("scenario 1: Detail-Seite Gold (43 rows)")
        await typeQuery("gold")
        await typeQuery("gold")
        await wait_for("document.querySelectorAll('.row').length", want=lambda x: x and x >= 1, timeout=8)
        rows = await js("document.querySelectorAll('.row').length")
        first = await txt(".row .name")
        print(f"  rows={rows} first={first!r}")
        await js("document.querySelector('.row').click()")
        await wait_for("document.querySelector('.pane-detail') ? 1 : 0", want=1, timeout=8)
        prs = await js("document.querySelectorAll('.pane-detail .pr').length")
        h2 = await txt(".d-head h2")
        print(f"  .pr={prs} h2={h2!r}")
        check("detail: 43 price rows", (prs or 0) == 43, f".pr={prs}")
        check("detail: title Gold", "Gold" in (h2 or ""), f"h2={h2!r}")
        await shot("01-detail-commodity-43-rows")

        # ===================================================================
        # 2) OBSOLET: Location-Lock (badge + gefilterte Detail, 30 Zeilen)
        # ===================================================================
        # Das Lock-Feature (":l"-Filter, .lock-badge, .lock-btn, Ortssperre-
        # Hinweis) wurde entfernt — die alten Szenario-Checks prüfen tote
        # Selektoren (3 obsolete Fails). Historischer Beleg:
        # docs/screenshots/v0.1.5/README.md (02-lock-filtered-detail.png,
        # Stub-Fixture). Der .lock-hint bleibt live als Filter-Hinweis.
        print("scenario 2: Location-Lock — OBSOLET (Feature entfernt, übersprungen)")

        # ===================================================================
        # 3) Marketplace: 15 + load-more → 30
        # ===================================================================
        print("scenario 3: Marketplace (15 + load-more 30)")
        await esc()
        await typeQuery("")
        await asyncio.sleep(0.5)
        await key("m", ctrlKey=True)
        await wait_for("document.querySelector('.pane-market') ? 1 : 0", want=1, timeout=8)
        await wait_for("document.querySelectorAll('.mlist .mrow').length", want=lambda x: x and x >= 1, timeout=8)
        mrows = await js("document.querySelectorAll('.mlist .mrow').length")
        await shot("03-market-15-load-more-30")
        check("market: 15 offers", mrows == 15, f"mrows={mrows}")
        await js("document.querySelector('.btn-more').click()")
        await asyncio.sleep(0.6)
        await wait_for("document.querySelectorAll('.mlist .mrow').length", want=lambda x: x and x >= 30, timeout=6)
        mrows2 = await js("document.querySelectorAll('.mlist .mrow').length")
        print(f"  mrows={mrows} -> {mrows2}")
        check("market: load-more -> 30", mrows2 == 30, f"mrows={mrows2}")

    proc.terminate()
    try:
        server.terminate()
    except Exception:
        pass
    print("\n--- LOGS ---")
    for l in logs[:50]:
        print("  ", l)
    print(f"\nRESULT: {PASS} passed, {FAIL} failed")
    sys.exit(1 if FAIL else 0)


if __name__ == "__main__":
    asyncio.run(main())
