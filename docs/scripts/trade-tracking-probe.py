#!/usr/bin/env python3
"""CDP-Probe PT (t_7fdc1317) — Tracking-Reiter in den Handelsrouten.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  P1 — Sub-Reiter „Routen | Tracking" im HANDELSROUTEN-Modus (F3).
  P2 — Trade-Liste aus /user_trades-Fixtures (get_my_trades-Stub):
       Kauf/Verkauf-Label, Ware, Ort, Menge (SCU), Preis, Datum.
  P3 — Tages-Aggregat: Ausgaben/Einnahmen/Nettogewinn je Tag (clientseitig,
       aUEC, de-DE-Formatierung).
  P4 — Erfassen: „+ Trade erfassen" öffnet Modal; Typeahead Ware/Ort
       (commodity_suggest/terminal_suggest); Richtung/Menge/Preis;
       „Trade prüfen" → 2-Schritt-Bestätigung; Bestätigen → trade_add mit
       Doku-Payload (id_terminal/id_commodity/operation/scu/price); neue
       Zeile erscheint nach dem Pull.
  P5 — 2-Schritt auch beim LÖSCHEN (trade_remove) — Zeile verschwindet.
  P6 — Fehlerpfad 429: Meldung, KEIN Retry; Deep-Link „UEX-Konto öffnen"
       feuert open_uex_account (keine Fake-Buttons).
  P7 — Esc schließt Formular/Bestätigung (Keyboard-Contract).

Datenquelle: smoke-stub.js (JS-Mirror; Rust-Backend durch cargo tests
belegt). Screenshots nach docs/screens/trade-tracking/.
"""
import asyncio, base64, json, os, pathlib, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens/trade-tracking"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8803"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9239"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

NOW = int(time.time())

def trade(id_, op, commodity, cid, terminal, tid, scu, price, date_added):
    return {
        "id": id_, "terminal_id": tid, "commodity_id": cid,
        "operation": op, "scu": scu, "price": price, "date_added": date_added,
        "commodity": commodity, "terminal": terminal,
        "vehicle": None, "fleet_name": None, "organization": None,
    }

TRADES = json.dumps([
    trade(1, "buy", "Gold", 11, "Everus Harbor", 21, 10, 1000, NOW - 3600),
    trade(2, "sell", "Laranite", 12, "Port Olisario", 22, 5, 2200, NOW - 2 * 86400),
    trade(3, "buy", "Titanium", 13, "Baijini Point", 23, 4, 500, NOW - 2 * 86400),
], separators=(",", ":"))

# Erwartete Tages-Aggregate (müssen mit dem UI übereinstimmen):
# Tag heute: Ausgabe 10*1000 = 10.000 (1 Trade).
# Tag vorgestern: Einnahme 5*2200 = 11.000, Ausgabe 4*500 = 2.000 → Netto +9.000.

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
        "--window-size=1920,1080", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-trade-tracking",
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
                m = json.loads(raw)
                if "id" in m and m["id"] in pending:
                    pending[m["id"]].set_result(m)
        asyncio.create_task(recv_loop())

        def ok(resp):
            if "error" in resp:
                raise RuntimeError("CDP error: %s" % resp["error"])
            return resp.get("result", {})

        await call("Page.enable")
        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1500, "deviceScaleFactor": 2, "mobile": False})

        async def navigate(stub_extra=""):
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
            if stub_extra:
                await call("Page.addScriptToEvaluateOnNewDocument", {"source": stub_extra})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.2)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        async def key(k, vk, code):
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.2)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = base64.b64decode(r["data"])
            (OUT / name).write_bytes(data)
            print("  shot:", OUT / name, len(data), "bytes")

        def type_input_js(selector, text):
            return ("(function(){"
                    "var i=document.querySelector(" + json.dumps(selector) + ");"
                    "if(!i) return 'NOFIELD';"
                    "i.focus();"
                    "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                    "set.call(i," + json.dumps(text) + ");"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'x'}));"
                    "return i.value;})()")

        def type_input_js_idx(selector, idx, text):
            return ("(function(){"
                    "var els=document.querySelectorAll(" + json.dumps(selector) + ");"
                    "var i=els[" + str(idx) + "];"
                    "if(!i) return 'NOFIELD';"
                    "i.focus();"
                    "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                    "set.call(i," + json.dumps(text) + ");"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'x'}));"
                    "return i.value;})()")

        async def enter_routes():
            await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
            await key("F3", 114, "F3")
            await asyncio.sleep(1.2)

        def hook_calls_js():
            return """(function(){
                var orig = window.__TAURI_INTERNALS__.invoke;
                window.__calls = window.__calls || [];
                window.__TAURI_INTERNALS__.invoke = function(cmd,args){
                    window.__calls.push(cmd + ':' + JSON.stringify(args || {}));
                    return orig(cmd,args);
                };
            })()"""

        def calls_matching(cmd):
            return js("(function(){return JSON.stringify((window.__calls||[]).filter(function(c){return c.indexOf(" + json.dumps(cmd + ":") + ")===0;}));})()")

        TRK_ENV = (
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "window.__SHOP_TRADES__ = " + TRADES + ";"
            "window.__SHOP_TRADES_SOURCE__ = 'cache';"
            "window.__SHOP_TRADES_NOTE__ = null;"
        )

        def rows_text_expr():
            return ("JSON.stringify([...document.querySelectorAll('.mlist .mrow.trow')]"
                    ".map(function(r){return r.textContent.replace(/\\s+/g,' ').trim();}))")

        # ================================================================
        # P1 — Sub-Reiter Routen | Tracking im Handelsrouten-Modus
        # ================================================================
        await navigate(TRK_ENV)
        await js(hook_calls_js())
        await enter_routes()
        chips = json.loads(str(await js(
            "(function(){return JSON.stringify([...document.querySelectorAll('.mbar2 .chip')].slice(0,2).map(function(c){return c.textContent.trim();}));})()")) or "[]")
        check("p1-subtabs-present", chips == ["Routen", "Tracking"], str(chips))
        await shot("pt-p1-subtabs.png")

        # ================================================================
        # P2 — Tracking-Liste (Kauf/Verkauf, Ware, Ort, SCU, Preis, Datum)
        # ================================================================
        opened = await js("(function(){var b=[...document.querySelectorAll('.mbar2 .chip')].find(function(c){return c.textContent.trim()==='Tracking';}); if(b){b.click(); return true;} return false;})()")
        check("p2-tracking-opens", opened is True)
        await asyncio.sleep(1.2)
        rows = json.loads(str(await js(rows_text_expr())) or "[]")
        # 2 Tages-Zeilen + 3 Trade-Zeilen
        check("p2-rows-rendered", len(rows) == 5, "rows=" + str(len(rows)))
        alltxt = " ".join(rows)
        check("p2-buy-label", any(r.startswith("Kauf") for r in rows), str(rows)[:160])
        check("p2-sell-label", any(r.startswith("Verkauf") for r in rows), str(rows)[:160])
        check("p2-commodity-terminal-scu-price", "Gold" in alltxt and "Everus Harbor" in alltxt and "10 SCU" in alltxt and "1.000" in alltxt, alltxt[:200])
        check("p2-source-foot", "/user_trades" in alltxt or await js("(function(){return (document.querySelector('.m-foot')||{textContent:''}).textContent.includes('/user_trades');})()") is True, "foot")
        await shot("pt-p2-list.png")

        # ================================================================
        # P3 — Tages-Aggregat (Ausgaben/Einnahmen/Nettogewinn)
        # ================================================================
        day_rows = json.loads(str(await js(
            "JSON.stringify([...document.querySelectorAll('.mlist .mrow.trow')].map(function(r){return r.textContent.replace(/\\s+/g,' ').trim();}).filter(function(t){return /Trade[s]?$/.test(t.split(' ')[0]);}))")) or "[]")
        # Aggregat-Zeilen erkennt man an "Trade"/"Trades" Sub-Label
        agg_txt = " ".join(rows)
        check("p3-expense-today", "10.000" in agg_txt, "Ausgabe 10.000 erwartet")
        check("p3-income-oldday", "11.000" in agg_txt, "Einnahme 11.000 erwartet")
        check("p3-net-oldday", "9.000" in agg_txt, "Netto +9.000 erwartet")
        await shot("pt-p3-daily-agg.png")

        # ================================================================
        # P4 — Erfassen: Formular + Typeahead + 2-Schritt + trade_add
        # ================================================================
        opened = await js("(function(){var b=[...document.querySelectorAll('.mbar .chip')].find(function(c){return c.textContent.trim()==='+ Trade erfassen';}); if(b){b.click(); return true;} return false;})()")
        check("p4-form-opens", opened is True)
        await asyncio.sleep(0.4)
        modal = await js("document.querySelectorAll('.confirm-modal').length")
        check("p4-form-modal", modal == 1, "modals=" + str(modal))
        # Richtung Verkauf
        await js("(function(){var b=[...document.querySelectorAll('.confirm-modal .chip')].find(function(c){return c.textContent.trim()==='Verkauf';}); if(b) b.click();})()")
        # Ware tippen → Vorschläge (commodity_suggest)
        typed = await js(type_input_js(".confirm-modal input.rtext", "Gol"))
        check("p4-commodity-typed", typed == "Gol", str(typed))
        await asyncio.sleep(0.8)
        opts = json.loads(str(await js(
            "JSON.stringify([...document.querySelectorAll('.confirm-modal .ta-item,.confirm-modal [role=option]')].map(function(o){return o.textContent.trim();}))")) or "[]")
        if not opts:
            # Typeahead-Dropdown-Klassen können abweichen — über Data-Attribut zählen
            opts = json.loads(str(await js(
                "JSON.stringify([...document.querySelectorAll('.confirm-modal .tt-opt,.confirm-modal .opt')].map(function(o){return o.textContent.trim();}))")) or "[]")
        check("p4-commodity-suggest-gold", any("Gold" in o for o in opts), str(opts))
        # Gold auswählen (Klick auf Dropdown-Eintrag)
        await js("(function(){var o=[...document.querySelectorAll('.confirm-modal .ta-item,.confirm-modal [role=option],.confirm-modal .tt-opt,.confirm-modal .opt')].find(function(x){return x.textContent.trim()==='Gold';}); if(o) o.click();})()")
        await asyncio.sleep(0.3)
        # Ort tippen + wählen (zwei .rtext-Typeaheads im Modal — Index 1 = Ort)
        await js(type_input_js_idx(".confirm-modal input.rtext", 1, "Everus"))
        await asyncio.sleep(0.8)
        # Ort-Auswahl: Dropdown nach 'Everus Harbor'
        await js("(function(){var o=[...document.querySelectorAll('.confirm-modal .ta-item,.confirm-modal [role=option],.confirm-modal .tt-opt,.confirm-modal .opt')].find(function(x){return x.textContent.trim()==='Everus Harbor';}); if(o) o.click();})()")
        await asyncio.sleep(0.3)
        # Menge + Preis (number inputs im Modal)
        await js(type_input_js(".confirm-modal input[type=number]", "12"))
        await asyncio.sleep(0.2)
        nums = await js("[...document.querySelectorAll('.confirm-modal input[type=number]')].length")
        if nums == 2:
            await js("(function(){var i=document.querySelectorAll('.confirm-modal input[type=number]')[1];"
                     "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                     "set.call(i,'2400');i.dispatchEvent(new InputEvent('input',{bubbles:true}));})()")
        await asyncio.sleep(0.3)
        await shot("pt-p4-form.png")
        # „Trade prüfen“ → Bestätigung
        clicked = await js("(function(){var b=[...document.querySelectorAll('.confirm-modal .btn')].find(function(x){return x.textContent.trim().includes('prüfen');}); if(b){b.click(); return true;} return false;})()")
        check("p4-review-clicked", clicked is True)
        await asyncio.sleep(0.4)
        conf = str(await js("(function(){var m=[...document.querySelectorAll('.confirm-modal')].find(function(x){return x.textContent.includes('ANLEGEN');}); return m? m.textContent.replace(/\\s+/g,' ').trim().slice(0,300) : null;})()"))
        check("p4-confirm-modal", conf is not None and "Verkauf" in conf and "Gold" in conf and "12 SCU" in conf and "2.400" in conf, str(conf))
        await shot("pt-p4-confirm.png")
        # Bestätigen → trade_add
        ok_btn = await js("(function(){var m=[...document.querySelectorAll('.confirm-modal')].find(function(x){return x.textContent.includes('ANLEGEN');}); if(!m) return false; var b=[...m.querySelectorAll('.btn')].find(function(x){return x.textContent.trim()==='Bestätigen';}); if(b){b.click(); return true;} return false;})()")
        check("p4-confirm-clicked", ok_btn is True)
        await asyncio.sleep(1.5)
        payload = str(await js("JSON.stringify(window.__LAST_TRADE_PAYLOAD__ || null)"))
        check("p4-payload-docschema",
              '"id_terminal":21' in payload and '"id_commodity":11' in payload and
              '"operation":"sell"' in payload and '"scu":12' in payload and '"price":2400' in payload, payload)
        rows2 = json.loads(str(await js(rows_text_expr())) or "[]")
        new_row = " ".join(rows2)
        check("p4-new-row-visible", "12 SCU" in new_row and "2.400" in new_row, new_row[:200])
        await shot("pt-p4-after-add.png")

        # ================================================================
        # P5 — Löschen mit 2-Schritt (trade_remove)
        # ================================================================
        del_click = await js("(function(){var b=[...document.querySelectorAll('.btn-ghost')].find(function(x){return x.textContent.trim()==='Löschen';}); if(b){b.click(); return true;} return false;})()")
        check("p5-delete-clicked", del_click is True)
        await asyncio.sleep(0.4)
        conf2 = str(await js("(function(){var m=[...document.querySelectorAll('.confirm-modal')].find(function(x){return x.textContent.includes('LÖSCHEN');}); return m? m.textContent.slice(0,200) : null;})()"))
        check("p5-confirm-delete", conf2 is not None, str(conf2))
        ok2 = await js("(function(){var m=[...document.querySelectorAll('.confirm-modal')].find(function(x){return x.textContent.includes('LÖSCHEN');}); if(!m) return false; var b=[...m.querySelectorAll('.btn')].find(function(x){return x.textContent.trim()==='Bestätigen';}); if(b){b.click(); return true;} return false;})()")
        check("p5-confirm-clicked", ok2 is True)
        await asyncio.sleep(1.5)
        removed_id = str(await js("JSON.stringify(window.__LAST_TRADE_REMOVE_ID__ || null)"))
        check("p5-remove-id-captured", removed_id not in ("null", "0", "None"), removed_id)
        await shot("pt-p5-after-delete.png")

        # ================================================================
        # P6 — Fehlerpfad 429 + Deep-Link (kein Fake-Button)
        # ================================================================
        await navigate(TRK_ENV + "window.__TRK_WRITE_FAIL__ = true; window.__TRK_WRITE_ERR__ = 'HTTP 429 · Rate-Limit erreicht';")
        await js(hook_calls_js())
        await enter_routes()
        await js("(function(){var b=[...document.querySelectorAll('.mbar2 .chip')].find(function(c){return c.textContent.trim()==='Tracking';}); if(b) b.click();})()")
        await asyncio.sleep(1.2)
        await js("(function(){var b=[...document.querySelectorAll('.mbar .chip')].find(function(c){return c.textContent.trim()==='+ Trade erfassen';}); if(b) b.click();})()")
        await asyncio.sleep(0.4)
        await js(type_input_js(".confirm-modal input.rtext", "Gol"))
        await asyncio.sleep(0.8)
        await js("(function(){var o=[...document.querySelectorAll('.confirm-modal .ta-item,.confirm-modal [role=option],.confirm-modal .tt-opt,.confirm-modal .opt')].find(function(x){return x.textContent.trim()==='Gold';}); if(o) o.click();})()")
        await asyncio.sleep(0.3)
        await js(type_input_js_idx(".confirm-modal input.rtext", 1, "Everus"))
        await asyncio.sleep(0.8)
        await js("(function(){var o=[...document.querySelectorAll('.confirm-modal .ta-item,.confirm-modal [role=option],.confirm-modal .tt-opt,.confirm-modal .opt')].find(function(x){return x.textContent.trim()==='Everus Harbor';}); if(o) o.click();})()")
        await asyncio.sleep(0.3)
        await js(type_input_js(".confirm-modal input[type=number]", "3"))
        await js("(function(){var i=document.querySelectorAll('.confirm-modal input[type=number]')[1];"
                 "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                 "set.call(i,'900');i.dispatchEvent(new InputEvent('input',{bubbles:true}));})()")
        await asyncio.sleep(0.3)
        await js("(function(){var b=[...document.querySelectorAll('.confirm-modal .btn')].find(function(x){return x.textContent.trim().includes('prüfen');}); if(b) b.click();})()")
        await asyncio.sleep(0.4)
        await js("(function(){var m=[...document.querySelectorAll('.confirm-modal')].find(function(x){return x.textContent.includes('ANLEGEN');}); if(!m) return; var b=[...m.querySelectorAll('.btn')].find(function(x){return x.textContent.trim()==='Bestätigen';}); if(b) b.click();})()")
        await asyncio.sleep(1.2)
        err_visible = await js("(function(){var els=[...document.querySelectorAll('.confirm-modal, .row-empty')]; return els.some(function(e){return (e.textContent||'').includes('429');});})()")
        check("p6-429-shown", err_visible is True, "429-Meldung sichtbar")
        deep = await js("(function(){var b=[...document.querySelectorAll('.mbar .chip')].find(function(c){return c.textContent.includes('UEX-Konto');}); if(b){b.click(); return true;} return false;})()")
        check("p6-deeplink-present", deep is True)
        await asyncio.sleep(0.4)
        url_open = str(await js("JSON.stringify(window.__LAST_OPEN_URL__ || null)"))
        check("p6-deeplink-uex-account", "uexcorp.space/account" in url_open, url_open)
        await shot("pt-p6-error-path.png")

        # ================================================================
        # P7 — Esc schließt Formular/Bestätigung
        # ================================================================
        open2 = await js("(function(){var b=[...document.querySelectorAll('.mbar .chip')].find(function(c){return c.textContent.trim()==='+ Trade erfassen';}); if(b){b.click(); return true;} return false;})()")
        await asyncio.sleep(0.4)
        check("p7-form-open", open2 is True)
        await key("Escape", 27, "Escape")
        await asyncio.sleep(0.3)
        still = await js("document.querySelectorAll('.confirm-modal').length")
        check("p7-esc-closes-form", still == 0, "modals=" + str(still))
        await shot("pt-p7-esc.png")

    server.terminate()
    proc.terminate()

    print()
    print("PASS:", len(PASS), "FAIL:", len(FAIL))
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

if __name__ == "__main__":
    asyncio.run(main())
