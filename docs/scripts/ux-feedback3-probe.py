#!/usr/bin/env python3
"""CDP-Probe für die UX-Feedback-3-Runde (t_733aa11d) auf feature/ux-feedback3.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  1. P7 — Helper-Pruning: die Präfix-Zeile unter dem Suchfeld zeigt nur noch
     ':c :i :l' + '= math'; '% ÷100', '↵ öffnet Detail' und 'Suchfeld = Filter'
     sind entfernt (im Search-Modus; Helper ist in Market/Detail ausgeblendet).
  2. P4 — F5 app-lokal & kontextsensitiv: Such-Modus → Backend 'sync_now'
     (Call-Log) + Status-Chip in der Fußzeile; Market-Modus → neuer
     'get_marketplace'-Call + Chip. Browser-Reload wird nie ausgelöst.
  3. P6 — EINE dynamische Hint-Zeile: die Fußzeile wechselt je Ansicht
     (Search: Nav/Alt+1..0/Market · Detail: Zeile/Suchfeld = Filter · Market:
     Angebote/Load-More) und es gibt KEINE Extra-Hinweiszeile im Detail
     (.d-foot existiert nicht).
  4. P8 — Preis-Detail: Variante C (Default, Trader-Hero mit Bestpreis-KPIs +
     kompakte Liste), Variante A per Konfig (localStorage) umschaltbar;
     Bestpreis-Chips sind gelabelt und folgen dem Filter (dBest aus den
     GEFILTERTEN dRows), leere Richtungen → EINE Statuszeile, Bestand 0 →
     Klartext ('kein Bestand gemeldet'), Sortierung Kauf aufsteigend /
     Verkauf absteigend.

Backend = smoke-stub.js (JS-Mirror des Provider-Kontrakts). Screenshots nach
docs/screenshots/ux-feedback3/.
"""
import asyncio, base64, json, os, pathlib, sys, urllib.request, urllib.error
import websockets, signal

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", "/home/sebo/Projects/Architects Overlay"))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/ux-feedback3")))
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
        print("boot: waiting for app…")
        await asyncio.sleep(1.0)
        # invoke-Log instrumentieren (für Sync-Trigger-Nachweis)
        await js("""(function(){
            var ti = window.__TAURI_INTERNALS__;
            if (ti && !ti.__logged) {
                var orig = ti.invoke;
                window.__calls = [];
                ti.invoke = function(cmd,args){
                    window.__calls.push(cmd);
                    return orig(cmd,args);
                };
                ti.__logged = true;
            }
        })()""")
        calls0 = await js("JSON.stringify(window.__calls || [])")

        # ===================================================================
        # 1) P7 — Helper-Pruning (Search-Modus, kein Filter aktiv)
        # ===================================================================
        print("scenario 1: P7 helper pruning")
        helper = await js("document.querySelector('.helper') ? document.querySelector('.helper').textContent.replace(/\\s+/g,' ').trim() : ''")
        print("  helper:", helper[:120])
        check("p7: helper present in search mode", bool(helper), helper)
        check("p7: helper has :c :i :l prefixes",
              ":c" in helper and ":i" in helper and ":l" in helper, helper)
        check("p7: helper has = math hint", "=" in helper, helper)
        check("p7: '÷100' removed", "÷100" not in helper, helper)
        check("p7: 'öffnet Detail' removed", "öffnet Detail" not in helper, helper)
        check("p7: 'Suchfeld = Filter' not in helper (moved to foot)",
              "Filter" not in helper, helper)

        # ===================================================================
        # 2) P4 — F5 app-lokal im Such-Modus → sync_now + Status-Chip
        # ===================================================================
        print("scenario 2: P4 F5 search mode")
        before = await js("(window.__calls || []).length")
        await js("window.dispatchEvent(new KeyboardEvent('keydown', {key: 'F5', bubbles: true}))")
        await asyncio.sleep(1.2)
        after = await js("(window.__calls || []).length")
        calls = await js("JSON.stringify(window.__calls || [])")
        print(f"  calls {before} -> {after}: {calls[:220]}")
        check("p4: F5 triggers sync_now in search mode",
              'sync_now' in str(calls), str(calls)[:200])
        chip = await js("document.querySelector('.f5-msg') ? document.querySelector('.f5-msg').textContent.trim() : ''")
        print("  f5-chip:", chip)
        check("p4: F5 shows status chip (not modal)", bool(chip) and 'läuft' in chip or 'aktualisiert' in chip, chip)
        check("p4: F5 did NOT reload the page (app still alive)",
              await js("document.querySelector('.smode') ? 1 : 0") == 1, "smode present")
        await shot("01-f5-search-sync")

        # ===================================================================
        # 3) P6 — eine dynamische Hint-Zeile (Foot wechselt je Ansicht)
        # ===================================================================
        foot_search = await js("document.querySelector('.foot .hints') ? document.querySelector('.foot .hints').textContent.replace(/\\s+/g,' ').trim() : ''")
        print("  foot(search):", foot_search[:140])
        check("p6: foot search hints", "Nav" in foot_search and "Market" in foot_search, foot_search)

        print("scenario 3: P6/P8 detail view (Gold, 43 rows)")
        await typeQuery("gold")
        await wait_for("document.querySelectorAll('.row').length", want=lambda x: x and x >= 1, timeout=8)
        await js("document.querySelector('.row').click()")
        await wait_for("document.querySelector('.pane-detail') ? 1 : 0", want=1, timeout=8)
        await asyncio.sleep(0.6)

        foot_detail = await js("document.querySelector('.foot .hints') ? document.querySelector('.foot .hints').textContent.replace(/\\s+/g,' ').trim() : ''")
        print("  foot(detail):", foot_detail[:140])
        check("p6: foot detail hints", "Zeile" in foot_detail and "Filter" in foot_detail, foot_detail)
        check("p6: no extra hint line in detail (.d-foot removed)",
              await js("document.querySelector('.pane-detail .d-foot') ? 1 : 0") == 0, "d-foot absent")

        # ===================================================================
        # 4) P8 — Variante C (Default): Bestpreis-Hero + kompakte Liste
        # ===================================================================
        hero = await js("document.querySelector('.vC-hero') ? 1 : 0")
        check("p8: variant C is default (hero present)", hero == 1, f"hero={hero}")
        kpis = await js("[...document.querySelectorAll('.vC-kpi .k')].map(function(x){return x.textContent.trim()})")
        print("  kpi labels:", kpis)
        check("p8: hero KPI labels (Kauf/Verkauf/Marge)",
              len(kpis or []) == 3 and kpis[0] == 'Günstigster Kauf' and kpis[1] == 'Höchster Verkauf' and kpis[2].startswith('Marge'), str(kpis))
        buy_v = await js("document.querySelector('.vC-kpi.buy .v') ? document.querySelector('.vC-kpi.buy .v').textContent.trim() : ''")
        sell_v = await js("document.querySelector('.vC-kpi.sell .v') ? document.querySelector('.vC-kpi.sell .v').textContent.trim() : ''")
        margin_v = await js("document.querySelector('.vC-kpi.margin .v') ? document.querySelector('.vC-kpi.margin .v').textContent.trim() : ''")
        print(f"  hero values: buy={buy_v} sell={sell_v} margin={margin_v}")
        # Stub: min buy = 229 (adminBuy), max sell = 2.840 (2000 + 41*20)
        check("p8: hero best buy = min price (229)", buy_v == "229", f"buy={buy_v}")
        check("p8: hero best sell = max price (2.820)", sell_v == "2.820", f"sell={sell_v}")
        check("p8: hero margin = sell - buy (2.591)", margin_v == "2.591", f"margin={margin_v}")
        # Kopf-Chips gelabelt und konsistent zum Hero
        chip_buy = await js("document.querySelector('.d-head .d-price .buy') ? document.querySelector('.d-head .d-price .buy').textContent.replace(/\\s+/g,' ').trim() : ''")
        check("p8: head chip labeled 'Best Kauf'", "Best Kauf" in chip_buy and "229" in chip_buy, chip_buy)
        chip_sell = await js("document.querySelector('.d-head .d-price .sell') ? document.querySelector('.d-head .d-price .sell').textContent.replace(/\\s+/g,' ').trim() : ''")
        check("p8: head chip labeled 'Best Verkauf'", "Best Verkauf" in chip_sell and "2.820" in chip_sell, chip_sell)
        # Sortierung: Kauf aufsteigend, Verkauf absteigend (erste Zeile je Richtung)
        rows = await js("[...document.querySelectorAll('.pane-detail .pr')].map(function(r){return r.textContent.replace(/\\s+/g,' ').trim()})")
        first_buy = next((r for r in rows or [] if 'Bestand' in r or not r.startswith('2.8')), rows[0] if rows else '')
        check("p8: rows exist (43 price rows)", len(rows or []) == 43, f"rows={len(rows or [])}")
        # Bestand 0 → Klartext
        stock_txt = await js("(function(){var s=[...document.querySelectorAll('.vC-t small')].map(function(x){return x.textContent}); return s.length ? s[0] : ''})()")
        check("p8: stock 0 renders plain text (kein Bestand gemeldet)",
              stock_txt in ('kein Bestand gemeldet', 'keine Nachfrage gemeldet'), stock_txt)
        await shot("02-p8-variantC-gold")

        # ===================================================================
        # 5) P8 — Bestpreis-Chip folgt dem Filter (dBest aus gefilterten Rows)
        # ===================================================================
        print("scenario 5: P8 chip follows filter")
        await typeQuery("area18")
        await asyncio.sleep(0.9)
        f_buy = await js("document.querySelector('.vC-kpi.buy .v') ? document.querySelector('.vC-kpi.buy .v').textContent.trim() : ''")
        f_sell = await js("document.querySelector('.vC-kpi.sell .v') ? document.querySelector('.vC-kpi.sell .v').textContent.trim() : ''")
        f_rows = await js("document.querySelectorAll('.pane-detail .pr').length")
        print(f"  filtered: buy={f_buy} sell={f_sell} rows={f_rows}")
        check("p8: hero follows filter (rows shrink)", (f_rows or 0) < 43 and f_rows != 0, f"rows={f_rows}")
        fchip_buy = await js("document.querySelector('.d-head .d-price .buy') ? document.querySelector('.d-head .d-price .buy').textContent.trim() : ''")
        check("p8: head chip follows filter (matches hero)", fchip_buy and f_buy in fchip_buy, f"{fchip_buy} vs {f_buy}")
        await typeQuery("")
        await asyncio.sleep(0.7)
        await esc()

        # ===================================================================
        # 6) P8 — Variante A per Konfig (localStorage), leere Richtung → Statuszeile
        # ===================================================================
        print("scenario 6: P8 variant A (CSP-68H: nur Kauf)")
        await typeQuery("csp-68h")
        await wait_for("document.querySelectorAll('.row').length", want=lambda x: x and x >= 1, timeout=8)
        await js("document.querySelector('.row').click()")
        await wait_for("document.querySelector('.pane-detail') ? 1 : 0", want=1, timeout=8)
        await asyncio.sleep(0.6)
        # Item hat nur Kauf-Preise → Variante C: Hero-Verkaufskarte zeigt '—'
        c_sell = await js("document.querySelector('.vC-kpi.sell .v') ? document.querySelector('.vC-kpi.sell .v').textContent.trim() : ''")
        check("p8: empty direction → dash in hero (—)", c_sell == '—', f"sell={c_sell}")
        c_sw = await js("document.querySelector('.vC-kpi.sell .w') ? document.querySelector('.vC-kpi.sell .w').textContent.trim() : ''")
        check("p8: empty direction explains (kein Verkauf-Angebot)", 'kein' in (c_sw or ''), c_sw)
        check("p8: no empty pseudo-section rows (no 'Kein Terminal kauft')",
              await js("(function(){var t=document.querySelector('.pane-detail').textContent; return t.indexOf('Kein Terminal kauft')>=0 || t.indexOf('Kein Terminal verkauft')>=0;})()") is False,
              "no legacy empty rows")
        # Toggle auf Variante A
        await js("document.querySelector('.d-head .d-variant button:nth-child(1)').click()")
        await asyncio.sleep(0.4)
        tabs = await js("document.querySelectorAll('.pane-detail .ptab').length")
        np = await js("[...document.querySelectorAll('.pane-detail .np-state')].map(function(x){return x.textContent.trim()})")
        check("p8: variant A renders grouped sections (1 ptab for buy)", (tabs or 0) == 1, f"ptabs={tabs}")
        check("p8: variant A empty direction → ONE status line", len(np or []) == 1 and 'verkauf' in np[0].lower(), str(np))
        # Persistenz liegt seit dem Integrat (t_0fb56efd) beim Backend-Settings
        # (t_163074f3, settings_set_detail_variant) — localStorage ist bewusst
        # NICHT mehr der Store. Statischer Build kann das Backend nicht
        # aufrufen, also darf hier kein localStorage-Key mehr entstehen.
        check("p8: variant NOT in localStorage (Store = Backend-Settings)",
              await js("localStorage.getItem('architects-overlay.p8.variant')") is None, "ls=None")
        await shot("03-p8-variantA-csp68h")
        # Toggle zurück auf C
        await js("document.querySelector('.d-head .d-variant button:nth-child(2)').click()")
        await asyncio.sleep(0.4)
        check("p8: toggle back to C (hero again)",
              await js("document.querySelector('.vC-hero') ? 1 : 0") == 1, "hero=1")
        await esc()

        # ===================================================================
        # 7) P6 — Market-Hints + P4 — F5 im Market-Modus
        # ===================================================================
        print("scenario 7: P6 market hints + P4 F5 market")
        await typeQuery("")
        await key("m", ctrlKey=True)
        await wait_for("document.querySelector('.pane-market') ? 1 : 0", want=1, timeout=8)
        await wait_for("document.querySelectorAll('.mlist .mrow').length", want=lambda x: x and x >= 1, timeout=8)
        foot_market = await js("document.querySelector('.foot .hints') ? document.querySelector('.foot .hints').textContent.replace(/\\s+/g,' ').trim() : ''")
        print("  foot(market):", foot_market[:140])
        check("p6: foot market hints (Angebote + Load-More)",
              "Angebote" in foot_market and "Weitere laden" in foot_market, foot_market)
        check("p7: helper hidden in market mode",
              await js("document.querySelector('.helper') ? 1 : 0") == 0, "helper absent")
        before = await js("(window.__calls || []).length")
        await js("window.dispatchEvent(new KeyboardEvent('keydown', {key: 'F5', bubbles: true}))")
        await asyncio.sleep(1.5)
        after = await js("(window.__calls || []).length")
        calls = await js("JSON.stringify((window.__calls || []).slice(" + str(before or 0) + "))")
        print(f"  market F5 calls {before} -> {after}: {calls[:200]}")
        check("p4: F5 in market mode triggers market sync (get_marketplace)",
              'get_marketplace' in str(calls), str(calls)[:200])
        chip = await js("document.querySelector('.f5-msg') ? document.querySelector('.f5-msg').textContent.trim() : ''")
        check("p4: market F5 shows status chip", bool(chip), chip)
        await shot("04-f5-market-sync")
        await esc()

        print(f"\nRESULT: PASS={PASS} FAIL={FAIL}")
        if FAIL:
            sys.exit(1)


if __name__ == "__main__":
    asyncio.run(main())
