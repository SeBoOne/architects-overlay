#!/usr/bin/env python3
"""CDP-Probe QY (t_07edb03d) — Trade-Log-Fixes.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  Q1 — Sub-Reiter heißen „Routen | Trade Log" (umbenannt von „Tracking").
  Q2 — Deep-Link-Button „Trade Log auf UEX öffnen ↗" feuert
       open_uex_trade_log → https://uexcorp.space/trade/home.
  Q3 — Formular-Layout: Felder nutzen die volle Breite des Popup-Bodys,
       größere Labels (≥ 10.5px), vertikaler Abstand zwischen den Feldern,
       Modal zentriert innerhalb der Overlay-Grenzen.
  Q4 — Cross-Autocomplete richtungskorrekt (Spielersicht): Kauf + Gold →
       nur Orte, die Gold VERKAUFEN (Everus Harbor, nicht Port Olisario);
       Verkauf + Gold → nur Orte, die Gold KAUFEN. Umgekehrt: Ort gewählt →
       nur dort handelbare Waren.
  Q5 — Einseitig beendeter Deal (date_closed_client ≠ 0, date_closed = 0 —
       Live-Befund: end_negotiation_* liefert UEX nicht): Composer
       AUSGEBLENDET, Hinweis „Trade beendet — Verlauf nur lesbar", Verlauf
       bleibt lesbar. Backend-Ablehnung durch cargo test
       send_message_rejects_onesided_ended_negotiation belegt.

Datenquelle: smoke-stub.js (JS-Mirror; Rust-Seite durch cargo tests belegt).
Screenshots nach docs/screens/qy-tradelog/.

Self-contained: startet statischen Server (build/) + headless Chromium.
"""
import asyncio, base64, json, os, pathlib, sys, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens/qy-tradelog"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8805"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9243"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

NOW = int(time.time())

def negotiation(id_, title, slug, is_advertiser, price, extra=None):
    n = {
        "id": id_, "listing_id": 4000 + id_, "hash": f"h-{id_}",
        "price": price, "unit": "unit", "currency": "UEC",
        "deal_value": None, "deal_value_currency": "",
        "listing_title": title, "listing_slug": slug,
        "advertiser_name": "Semih Balci", "advertiser_username": "sebo_one",
        "client_name": "John Doe", "client_username": "johndoe",
        "is_listing_advertiser": is_advertiser,
        "date_added": NOW - 10 * 86400,
        "date_modified": NOW - id_ * 3600,
        "date_closed": None,
    }
    n.update(extra or {})
    return n

def msg(id_, hash_, username, text, date_added, event=None):
    return {
        "id": id_, "id_listing": 4001, "id_negotiation": 9,
        "event": event, "message": text if event is None else None,
        "listing_title": "Drake Nettle S | Used", "listing_slug": "drake-nettle-s-used",
        "negotiation_hash": hash_, "user_name": username.title(),
        "user_username": username, "api_name": "uex-web",
        "date_added": date_added, "date_read": None,
    }

# Neueste Aktivität zuerst → der EINSEITIG beendete Deal steht oben.
DEALS = json.dumps([
    negotiation(9, "Drake Nettle S | Used", "drake-nettle-s-used", True, 4500000.0,
                {"date_modified": NOW + 86400,
                 "date_closed": None,
                 "date_closed_client": NOW - 3600}),  # EINSEITIG: nur Kunde hat abgeschlossen
    negotiation(8, "ARGO MPUV Cargo", "argo-mpuv-cargo", False, 1200000.0,
                {"date_modified": NOW}),
], separators=(",", ":"))

MSGS = json.dumps({
    "h-9": [
        msg(1, "h-9", "johndoe", "Zahlung ist raus, danke!", NOW - 7200),
        msg(2, "h-9", "sebo_one", "Passt — viel Freude damit.", NOW - 3600),
    ],
}, separators=(",", ":"))

# terminal_prices-Stammdaten-Fixture (Spielersicht): buy_price>0 = Ort VERKAUFT
# die Ware (Spieler kann dort kaufen), sell_price>0 = Ort KAUFT sie.
PRICES = json.dumps([
    {"commodity_id": 11, "terminal_id": 21, "buy_price": 1000, "sell_price": 0},    # Everus VERKAUFT Gold
    {"commodity_id": 11, "terminal_id": 22, "buy_price": 0, "sell_price": 1050},    # Olisario KAUFT Gold
    {"commodity_id": 12, "terminal_id": 22, "buy_price": 800, "sell_price": 0},     # Olisario VERKAUFT Laranite
    {"commodity_id": 12, "terminal_id": 23, "buy_price": 0, "sell_price": 820},     # Baijini KAUFT Laranite
    {"commodity_id": 13, "terminal_id": 23, "buy_price": 500, "sell_price": 0},     # Baijini VERKAUFT Titanium
    {"commodity_id": 13, "terminal_id": 21, "buy_price": 0, "sell_price": 510},     # Everus KAUFT Titanium
], separators=(",", ":"))

BASE_ENV = (
    "window.__stubState.uex_api_key_set = true;"
    "window.__stubState.uex_secret_key_set = true;"
    "window.__QY_TRADE_PRICES__ = " + PRICES + ";"
)

TRK_ENV = BASE_ENV + (
    "window.__SHOP_TRADES__ = [];"
    "window.__SHOP_TRADES_SOURCE__ = 'cache';"
    "window.__SHOP_TRADES_NOTE__ = null;"
)

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
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-qy-tradelog",
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

        VK = {"F2": (113, "F2"), "F3": (114, "F3")}
        async def key(k):
            vk, code = VK[k]
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

        async def click_chip(label, scope="document"):
            return await js(
                "(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()")

        def has_text_expr(sel, text):
            return ("(function(){var els=[...document.querySelectorAll(" + json.dumps(sel) + ")];"
                    "return els.some(function(e){return (e.textContent||'').includes(" + json.dumps(text) + ");});})()")

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
            await key("F3")
            await asyncio.sleep(1.2)

        async def enter_market():
            await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
            if await js("!!document.querySelector('input.q')"):
                setv = ("(function(){"
                        "var i=document.querySelector('input.q');"
                        "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                        "set.call(i,'WARM');"
                        "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'WARM'}));"
                        "return i.value;})()")
                await js(setv)
                await asyncio.sleep(0.6)
                await js("(function(){var i=document.querySelector('input.q');"
                         "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                         "set.call(i,'');"
                         "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));})()")
            await js("(function(){var i=document.querySelector('input.q'); if(i){i.focus();}})()")
            await key("F2")
            await asyncio.sleep(1.2)

        def hook_calls_js():
            return """(function(){
                var orig = window.__TAURI_INTERNALS__.invoke;
                window.__calls = window.__calls || [];
                window.__TAURI_INTERNALS__.invoke = function(cmd,args){
                    window.__calls.push(cmd + ':' + JSON.stringify(args || {}));
                    return orig(cmd,args);
                };
            })"""

        async def open_form():
            opened = await js("(function(){var b=[...document.querySelectorAll('.mbar .chip')].find(function(c){return c.textContent.trim()==='+ Trade erfassen';}); if(b){b.click(); return true;} return false;})()")
            await asyncio.sleep(0.4)
            return opened

        async def ta_options():
            for sel in (".trk-modal .rta-item",):
                opts = json.loads(str(await js(
                    "JSON.stringify([...document.querySelectorAll(" + json.dumps(sel) + ")].map(function(o){return o.textContent.trim();}))")) or "[]")
                if opts:
                    return opts
            return []

        async def pick_option(label):
            await js("(function(){var o=[...document.querySelectorAll('.trk-modal .rta-item')].find(function(x){return x.textContent.trim()===" + json.dumps(label) + "}); if(o) o.click();})()")
            await asyncio.sleep(0.3)

        # =================================================================
        # Q1 — Sub-Reiter „Routen | Trade Log"
        # =================================================================
        await navigate(TRK_ENV)
        await js(hook_calls_js() + "()")
        await enter_routes()
        chips = json.loads(str(await js(
            "(function(){return JSON.stringify([...document.querySelectorAll('.mbar2 .chip')].slice(0,2).map(function(c){return c.textContent.trim();}));})()")) or "[]")
        check("q1-subtabs-tradelog", chips == ["Routen", "Trade Log"], str(chips))
        await shot("qy-q1-subtabs.png")

        # =================================================================
        # Q2 — Deep-Link „Trade Log auf UEX öffnen ↗" → /trade/home
        # =================================================================
        tab = await click_chip("Trade Log")
        check("q2-tradelog-opens", tab is True)
        await asyncio.sleep(1.2)
        label = str(await js(
            "(function(){var c=[...document.querySelectorAll('.chip')].find(function(x){return (x.textContent||'').includes('Trade Log auf UEX');}); return c?c.textContent.trim():null;})()"))
        check("q2-button-label", label == "Trade Log auf UEX öffnen ↗", str(label))
        await js("window.__calls = []")
        clicked = await js(
            "(function(){var c=[...document.querySelectorAll('.chip')].find(function(x){return (x.textContent||'').includes('Trade Log auf UEX');}); if(c){c.click(); return true;} return false;})()")
        check("q2-button-clicked", clicked is True)
        await asyncio.sleep(0.6)
        url_open = str(await js("JSON.stringify(window.__LAST_OPEN_URL__ || null)"))
        check("q2-url-trade-home", "uexcorp.space/trade/home" in url_open, url_open)
        await shot("qy-q2-button.png")

        # =================================================================
        # Q3 — Formular-Layout (volle Breite, Labels, Rhythmus, zentriert)
        # =================================================================
        opened = await open_form()
        check("q3-form-opens", opened is True)
        await asyncio.sleep(0.4)
        layout = str(await js("""(function(){
            var body=document.querySelector('.trk-modal .trk-body');
            var fields=[...document.querySelectorAll('.trk-modal .trk-field')];
            if(!body||fields.length<5) return null;
            var bb=body.getBoundingClientRect();
            var infos=fields.map(function(f){
                var inp=f.querySelector('input, .tt-wrap input')||f.querySelector('input');
                var lbl=f.querySelector('.trk-lbl');
                var fr=f.getBoundingClientRect();
                var w=null, fs=null;
                if(lbl){var cs=getComputedStyle(lbl); fs=parseFloat(cs.fontSize);}
                var target=inp||f;
                var tb=target.getBoundingClientRect();
                w=Math.round(tb.width);
                return {fieldTop:Math.round(fr.top), fieldBottom:Math.round(fr.bottom), width:w, labelPx:fs};
            });
            var modal=document.querySelector('.trk-modal');
            var mb=modal.getBoundingClientRect();
            return JSON.stringify({bodyW:Math.round(bb.width), fields:infos,
                modalTop:Math.round(mb.top), modalBottom:Math.round(mb.bottom),
                overlay:document.querySelector('.overlay')?true:false});
        })()"""))
        d = json.loads(layout) if layout and layout != "None" else None
        check("q3-layout-probe-data", d is not None, str(layout)[:120])
        if d:
            body_w = d["bodyW"]
            widths = [f["width"] for f in d["fields"]]
            check("q3-fields-full-width", all(w >= body_w * 0.9 for w in widths),
                  f"bodyW={body_w} widths={widths}")
            gaps = [d["fields"][i + 1]["fieldTop"] - d["fields"][i]["fieldBottom"] for i in range(len(d["fields"]) - 1)]
            check("q3-vertical-rhythm", all(g >= 8 for g in gaps), f"gaps={gaps}")
            labels = [f["labelPx"] for f in d["fields"] if f["labelPx"]]
            check("q3-labels-larger", all(fs >= 10.5 for fs in labels), f"labelPx={labels}")
            mt, mb_ = d["modalTop"], d["modalBottom"]
            check("q3-modal-in-bounds", mt >= 0 and mb_ <= 1500, f"top={mt} bottom={mb_}")
        await shot("qy-q3-form-layout.png")

        # =================================================================
        # Q4 — Cross-Autocomplete richtungskorrekt (beide Richtungen)
        # =================================================================
        # Q4a: Richtung Kauf (Default) + Ware Gold → Ort-Autocomplete nur
        # Orte, die Gold VERKAUFEN (Everus Harbor) — nicht Port Olisario.
        await js(type_input_js(".trk-modal input.rtext", "Gol"))
        await asyncio.sleep(0.8)
        await pick_option("Gold")
        await js(type_input_js_idx(".trk-modal input.rtext", 1, "o"))
        await asyncio.sleep(0.8)
        locs = await ta_options()
        check("q4a-buy-gold-locations", any("Everus Harbor" in o for o in locs) and not any("Port Olisario" in o for o in locs), str(locs))
        await shot("qy-q4a-buy-gold-locations.png")
        await pick_option("Everus Harbor")
        # Umgekehrt: Ort Everus + Kauf → nur Waren, die Everus VERKAUFT (Gold).
        await js(type_input_js(".trk-modal input.rtext", "Ti"))
        await asyncio.sleep(0.8)
        coms = await ta_options()
        check("q4b-buy-everus-commodities", not any("Titanium" in c for c in coms), str(coms))
        await js(type_input_js(".trk-modal input.rtext", "Go"))
        await asyncio.sleep(0.8)
        coms2 = await ta_options()
        check("q4b-everus-has-gold", any("Gold" in c for c in coms2), str(coms2))
        await pick_option("Gold")
        # Q4c: Richtung Verkauf + Gold → nur Orte, die Gold KAUFEN (Olisario).
        await js("(function(){var b=[...document.querySelectorAll('.trk-modal .chip')].find(function(c){return c.textContent.trim()==='Verkauf';}); if(b) b.click();})()")
        await asyncio.sleep(0.6)
        await js(type_input_js_idx(".trk-modal input.rtext", 1, "o"))
        await asyncio.sleep(0.8)
        locs2 = await ta_options()
        check("q4c-sell-gold-locations", any("Port Olisario" in o for o in locs2) and not any("Everus Harbor" in o for o in locs2), str(locs2))
        await shot("qy-q4c-sell-gold-locations.png")
        # Q4d: Richtung-Wechsel meldet sich am Backend mit korrekter operation
        # (letzte trade_location_suggest-Args tragen operation:sell + id_commodity:11).
        last_sug = str(await js(
            "(function(){var c=(window.__calls||[]).filter(function(x){return x.indexOf('trade_location_suggest:')===0;}); return c.length?c[c.length-1]:null;})()"))
        check("q4d-suggest-args-snakecase", last_sug and '"operation":"sell"' in last_sug and '"id_commodity":11' in last_sug, str(last_sug))
        # Q4e: Ort gewählt + Verkauf: Olisario KAUFT nur Gold (Laranite verkauft
        # Olisario selbst, Titanium handelt Olisario gar nicht).
        await pick_option("Port Olisario")
        await js(type_input_js(".trk-modal input.rtext", "Ti"))
        await asyncio.sleep(0.8)
        coms = await ta_options()
        check("q4e-sell-olisario-no-laranite", not any(("Laranite" in c or "Titanium" in c) for c in coms), str(coms))
        await shot("qy-q4e-sell-olisario-commodities.png")
        await js(type_input_js(".trk-modal input.rtext", "Go"))
        await asyncio.sleep(0.8)
        coms3 = await ta_options()
        check("q4f-sell-olisario-has-gold", any("Gold" in c for c in coms3), str(coms3))
        await shot("qy-q4f-sell-olisario-commodities.png")

        # =================================================================
        # Q5 — Einseitig beendeter Deal: read-only im Chat
        # =================================================================
        await navigate(BASE_ENV + (
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_NEGOTIATIONS__ = " + DEALS + ";"
            "window.__TRADE_MSGS__ = " + MSGS + ";"
        ))
        await js(hook_calls_js() + "()")
        await enter_market()
        tab = await click_chip("Trades")
        check("q5-trades-tab-opens", tab is True)
        await asyncio.sleep(1.5)
        rows = json.loads(str(await js(
            "JSON.stringify([...document.querySelectorAll('.mlist .mrow.trow')].map(function(r){return r.textContent.replace(/\\s+/g,' ').trim();}))")) or "[]")
        check("q5-deal-rows-rendered", len(rows) == 2, "rows=" + str(len(rows)))
        opened = await js("(function(){var r=document.querySelector('.mlist .mrow.trow'); if(r){r.click(); return true;} return false;})()")
        check("q5-deal-click", opened is True)
        await asyncio.sleep(1.5)
        # Composer AUSGEBLENDET, Hinweis da, Verlauf lesbar.
        composer = await js("document.querySelectorAll('.chat-input').length")
        check("q5-composer-hidden", composer == 0, "chat-input count=" + str(composer))
        hint = await js(has_text_expr(".mbar .chip", "Verlauf nur lesbar"))
        check("q5-read-only-hint", hint is True)
        ended = await js(has_text_expr(".mbar .chip", "Trade beendet"))
        check("q5-ended-label-onesided", ended is True)
        history = await js("document.querySelectorAll('.chat .chat-msg').length")
        check("q5-history-visible", history == 2, "msgs=" + str(history))
        await shot("qy-q5-onesided-readonly.png")
        # Offener Deal (zweiter) hat Composer weiterhin.
        back = await click_chip("← Deals")
        await asyncio.sleep(0.8)
        opened2 = await js("(function(){var rs=document.querySelectorAll('.mlist .mrow.trow'); var r=rs[rs.length-1]; if(r){r.click(); return true;} return false;})()")
        check("q5-open-deal-click", opened2 is True)
        await asyncio.sleep(1.5)
        composer2 = await js("document.querySelectorAll('.chat-input').length")
        check("q5-open-deal-has-composer", composer2 == 1, "chat-input count=" + str(composer2))
        await shot("qy-q5-open-deal-composer.png")

    proc.terminate()
    server.terminate()

    print()
    print("PASS:", len(PASS), "FAIL:", len(FAIL))
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

if __name__ == "__main__":
    asyncio.run(main())
