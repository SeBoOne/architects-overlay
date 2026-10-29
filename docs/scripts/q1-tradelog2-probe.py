#!/usr/bin/env python3
"""CDP-Probe Q1 (t_535a98fb) — Trade Log Welle 2.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  Q1 — Commodities bleiben in BEIDEN Richtungen immer wählbar (Gold,
       Janalite, Pitambu) — mit UND ohne gewählten Ort; der Ort-Filter
       priorisiert, schneidet aber nie ab.
  Q2 — Alle Formular-Felder (Typeahead + Zahlenfelder) nutzen die volle
       Popup-Breite (>= 90% des Popup-Bodys).
  Q3 — Buchhaltungs-Konvention: positive Beträge GRÜN (--ok), negative ROT
       (--alert) — Trade-Liste (Gesamtwert) + Tages-Aggregat.
  Q4 — Gesamtwert pro Trade (Menge × Preis) als eigene Zeile in der Liste.
  Q5 — EIN Button „Deal auf UEX öffnen ↗" im Trades-Detail, Klick öffnet
       https://uexcorp.space/marketplace/negotiate/hash/<hash>; unterer
       zweiter Button entfällt (auch bei beendetem Deal).

Datenquelle: smoke-stub.js (JS-Mirror; Rust-Seite durch cargo tests belegt).
Screenshots nach docs/screens/q1-tradelog2/.

Self-contained: startet statischen Server (build/) + headless Chromium.
"""
import asyncio, base64, json, os, pathlib, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens/q1-tradelog2"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8811"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9251"))
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
        "date_closed": None, "date_closed_client": None,
    }
    n.update(extra or {})
    return n

# Offener Deal (Composer sichtbar) + beendeter Deal (read-only-Fuß OHNE
# zweiten Button — Q5 prüft beide Zustände).
DEALS = json.dumps([
    negotiation(9, "Drake Nettle S | Used", "drake-nettle-s-used", True, 4500000.0,
                {"date_modified": NOW}),
    negotiation(8, "ARGO MPUV Cargo", "argo-mpuv-cargo", False, 1200000.0,
                {"date_modified": NOW - 3600,
                 "date_closed": NOW - 1800,
                 "date_closed_client": NOW - 1800}),
], separators=(",", ":"))

MSGS = json.dumps({"h-9": [], "h-8": []}, separators=(",", ":"))

# Q3/Q4: Trade-Fixtures — Kauf (Ausgabe, ROT, −) und Verkauf (Einnahme, GRÜN, +).
TRK = json.dumps([
    {"id": 1, "commodity": "Gold", "commodity_id": 11, "terminal": "Everus Harbor",
     "terminal_id": 21, "operation": "buy", "scu": 10, "price": 1200,
     "date_added": NOW - 3600},
    {"id": 2, "commodity": "Titanium", "commodity_id": 13, "terminal": "Baijini Point",
     "terminal_id": 23, "operation": "sell", "scu": 4, "price": 2500,
     "date_added": NOW - 7200},
], separators=(",", ":"))

# terminal_prices-Fixture: Everus verkauft Gold; Olisario kauft Gold.
PRICES = json.dumps([
    {"commodity_id": 11, "terminal_id": 21, "buy_price": 1200, "sell_price": 0},
    {"commodity_id": 11, "terminal_id": 22, "buy_price": 0, "sell_price": 1250},
], separators=(",", ":"))

ENV = (
    "window.__stubState.uex_api_key_set = true;"
    "window.__stubState.uex_secret_key_set = true;"
    "window.__QY_TRADE_PRICES__ = " + PRICES + ";"
    "window.__SHOP_TRADES__ = " + TRK + ";"
    "window.__SHOP_TRADES_SOURCE__ = 'live';"
    "window.__SHOP_TRADES_NOTE__ = null;"
    # Q1-Testwaren (Sebos Beispiele) zusätzlich zum QY-Pool.
    "globalThis.__trkComPool = ["
    "{ id: 11, name: 'Gold' }, { id: 12, name: 'Laranite' }, { id: 13, name: 'Titanium' },"
    "{ id: 31, name: 'Janalite' }, { id: 32, name: 'Pitambu' }];"
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
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-q1-tradelog2",
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

        async def click_chip(label):
            return await js(
                "(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()")

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

        async def open_form():
            opened = await js("(function(){var b=[...document.querySelectorAll('.mbar .chip')].find(function(c){return c.textContent.trim()==='+ Trade erfassen';}); if(b){b.click(); return true;} return false;})()")
            await asyncio.sleep(0.4)
            return opened

        async def ta_options():
            return json.loads(str(await js(
                "JSON.stringify([...document.querySelectorAll('.trk-modal .rta-item')].map(function(o){return o.textContent.trim();}))")) or "[]")

        async def set_direction(dir_label):
            await js("(function(){var b=[...document.querySelectorAll('.trk-modal .chip')].find(function(c){return c.textContent.trim()===" + json.dumps(dir_label) + "}); if(b) b.click();})()")
            await asyncio.sleep(0.5)

        def rgb(el):
            # Farbe via Inline-getComputedStyle im DOM-Kontext lesen.
            return el

        # =================================================================
        # Q1 — Commodities in beiden Richtungen immer wählbar
        # =================================================================
        await navigate(ENV)
        await enter_routes()
        tab = await click_chip("Trade Log")
        check("setup-tradelog-tab", tab is True)
        await asyncio.sleep(1.0)
        opened = await open_form()
        check("q1-form-opens", opened is True)
        await asyncio.sleep(0.5)

        async def com_options_for(query):
            await js(type_input_js_idx(".trk-modal input.rtext", 0, query))
            await asyncio.sleep(0.8)
            return await ta_options()

        # Q1a: Kauf, OHNE Ort — Gold, Janalite, Pitambu wählbar.
        for q, name in (("Go", "Gold"), ("Ja", "Janalite"), ("Pi", "Pitambu")):
            opts = await com_options_for(q)
            check(f"q1a-buy-no-terminal-{name.lower()}", any(name in o for o in opts), str(opts))

        # Q1b: Kauf, MIT Ort (Everus) — immer noch alle drei wählbar.
        await js(type_input_js_idx(".trk-modal input.rtext", 1, "Everus"))
        await asyncio.sleep(0.8)
        await js("(function(){var o=[...document.querySelectorAll('.trk-modal .rta-item')].find(function(x){return x.textContent.trim().includes('Everus');}); if(o) o.click();})()")
        await asyncio.sleep(0.6)
        for q, name in (("Go", "Gold"), ("Ja", "Janalite"), ("Pi", "Pitambu")):
            opts = await com_options_for(q)
            check(f"q1b-buy-terminal-{name.lower()}", any(name in o for o in opts), str(opts))

        # Q1c: Verkauf (Richtungswechsel) — immer noch alle drei.
        await set_direction("Verkauf")
        for q, name in (("Go", "Gold"), ("Ja", "Janalite"), ("Pi", "Pitambu")):
            opts = await com_options_for(q)
            check(f"q1c-sell-{name.lower()}", any(name in o for o in opts), str(opts))
        await shot("q1-commodities-selectable.png")

        # =================================================================
        # Q2 — Formular-Felder volle Breite
        # =================================================================
        layout = str(await js("""(function(){
            var body=document.querySelector('.trk-modal .trk-body');
            var fields=[...document.querySelectorAll('.trk-modal .trk-field')];
            if(!body||fields.length<5) return null;
            var bb=body.getBoundingClientRect();
            var widths=fields.map(function(f){
                var t=f.querySelector('.rta-input')||f.querySelector('input');
                return t?Math.round(t.getBoundingClientRect().width):null;
            });
            return JSON.stringify({bodyW:Math.round(bb.width), widths:widths});
        })()"""))
        d = json.loads(layout) if layout and layout != "None" else None
        check("q2-layout-probe-data", d is not None, str(layout)[:120])
        if d:
            widths = [w for w in d["widths"] if w]  # Richtung-Feld hat Buttons, kein input
            check("q2-fields-full-width", len(widths) == 4 and all(w >= d["bodyW"] * 0.9 for w in widths),
                  f"bodyW={d['bodyW']} widths={d['widths']}")
        await shot("q2-form-full-width.png")

        # =================================================================
        # Q3+Q4 — Farben (Buchhaltung) + Gesamtwert in Liste + Aggregat
        # =================================================================
        await js("(function(){var b=[...document.querySelectorAll('.chip')].find(function(c){return c.textContent.trim()==='Abbrechen (Esc)';}); if(b) b.click();})()")
        await asyncio.sleep(0.4)
        rows = json.loads(str(await js(
            "JSON.stringify([...document.querySelectorAll('.mlist .mrow.trow')].map(function(r){return {txt:r.textContent.replace(/\\s+/g,' ').trim(), html:r.innerHTML};}))")) or "[]")
        check("q4-trade-rows-rendered", len(rows) >= 2, "rows=" + str(len(rows)))
        buy_row = next((r for r in rows if "Everus Harbor" in r["txt"]), None)
        sell_row = next((r for r in rows if "Baijini Point" in r["txt"]), None)
        # Q4: Gesamtwert = Menge × Preis (10×1200=12000 / 4×2500=10000).
        check("q4-buy-total-12000", bool(buy_row) and "12.000" in buy_row["txt"], str(buy_row and buy_row["txt"]))
        check("q4-sell-total-10000", bool(sell_row) and "10.000" in sell_row["txt"], str(sell_row and sell_row["txt"]))
        colors = str(await js("""(function(){
            function col(el){return el?getComputedStyle(el).color:null;}
            var out={};
            [...document.querySelectorAll('.mlist .mrow.trow .m-price')].forEach(function(p){
                out[p.textContent.trim()]=col(p);
            });
            var aggs=[...document.querySelectorAll('.mlist .mrow.trow .m-meta .amt-pos, .mlist .mrow.trow .m-meta .amt-neg')].map(function(s){return {cls:s.className, c:col(s)};});
            out.__aggs=aggs;
            return JSON.stringify(out);
        })()"""))
        c = json.loads(colors) if colors and colors != "None" else {}
        def is_green(v): return v and v in ("rgb(107, 214, 148)", "rgb(149, 247, 189)")
        def is_red(v): return v and v == "rgb(255, 122, 92)"
        check("q3-buy-total-red", is_red(c.get("−12.000")), str(c.get("−12.000")))
        check("q3-sell-total-green", is_green(c.get("+10.000")), str(c.get("+10.000")))
        aggs = c.get("__aggs") or []
        negs = [a for a in aggs if "amt-neg" in a["cls"]]
        poss = [a for a in aggs if "amt-pos" in a["cls"]]
        check("q3-aggregate-expense-red", negs and all(is_red(a["c"]) for a in negs), str(negs))
        check("q3-aggregate-income-green", poss and all(is_green(a["c"]) for a in poss), str(poss))
        await shot("q3-colors-and-totals.png")

        # =================================================================
        # Q5 — Deal-Deep-Link (negotiate/hash), EIN Button
        # =================================================================
        await navigate(ENV + (
            "window.__SHOP_NEGOTIATIONS__ = " + DEALS + ";"
            "window.__TRADE_MSGS__ = " + MSGS + ";"
        ))
        await enter_market()
        tab = await click_chip("Trades")
        check("q5-trades-tab", tab is True)
        await asyncio.sleep(1.5)
        opened = await js("(function(){var r=document.querySelector('.mlist .mrow.trow'); if(r){r.click(); return true;} return false;})()")
        check("q5-deal-open", opened is True)
        await asyncio.sleep(1.2)
        chips = json.loads(str(await js(
            "JSON.stringify([...document.querySelectorAll('.mbar .chip')].map(function(c){return c.textContent.trim();}))")) or "[]")
        deal_chips = [x for x in chips if "UEX" in x]
        check("q5-single-deal-button", len(deal_chips) == 1 and deal_chips[0] == "Deal auf UEX öffnen ↗", str(chips))
        await js("window.__LAST_OPEN_URL__ = null")
        clicked = await js("(function(){var c=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()==='Deal auf UEX öffnen ↗';}); if(c){c.click(); return true;} return false;})()")
        check("q5-button-clicked", clicked is True)
        await asyncio.sleep(0.5)
        url_open = str(await js("JSON.stringify(window.__LAST_OPEN_URL__ || null)"))
        check("q5-url-negotiate-hash", "uexcorp.space/marketplace/negotiate/hash/h-9" in url_open, url_open)
        await shot("q5-deal-button.png")
        # Beendeter Deal: read-only-Fuß OHNE zweiten Button.
        back = await click_chip("← Deals")
        await asyncio.sleep(0.8)
        opened2 = await js("(function(){var rs=document.querySelectorAll('.mlist .mrow.trow'); var r=rs[rs.length-1]; if(r){r.click(); return true;} return false;})()")
        check("q5-ended-deal-open", opened2 is True)
        await asyncio.sleep(1.2)
        ended_info = json.loads(str(await js("""(function(){
            var chips=[...document.querySelectorAll('.mbar .chip')].map(function(c){return c.textContent.trim();});
            return JSON.stringify({chips:chips, uexBtns:chips.filter(function(x){return x.indexOf('UEX')>=0;}).length,
                composer:document.querySelectorAll('.chat-input').length});
        })()""")) or "{}")
        e = ended_info if isinstance(ended_info, dict) else (json.loads(ended_info) if ended_info and ended_info != "None" else {})
        check("q5-ended-read-only", e.get("composer") == 0 and "Verlauf nur lesbar" in " ".join(e.get("chips", [])), str(ended_info))
        # Q1 (t_535a98fb): nur EIN UEX-Button — der obere, immer sichtbar,
        # Label „Deal auf UEX öffnen"; der untere zweite ist ENTFERNT.
        check("q5-ended-no-second-button", e.get("uexBtns") == 1
              and "Deal auf UEX öffnen ↗" in " ".join(e.get("chips", []))
              and not any("Auf UEX öffnen" in ch for ch in e.get("chips", [])), str(ended_info))
        await shot("q5-ended-no-button.png")

    proc.terminate()
    server.terminate()

    print()
    print("PASS:", len(PASS), "FAIL:", len(FAIL))
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

if __name__ == "__main__":
    asyncio.run(main())
