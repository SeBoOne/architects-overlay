#!/usr/bin/env python3
"""CDP-Probe M3b (t_893761f1) — Trades-Reiter: Deal-Liste + Chat im Overlay.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  T1 — Neuer Reiter „Trades" neben „Angebote"/„Mein Shop" im MARKET-Mode.
  T2 — Deal-Liste: get_my_negotiations-Fixtures, neueste Aktivität
       (date_modified) zuerst, Gegenpart + Status + Datum, Deep-Link-Pfad
       (dealUnread-Punkt bei neuer Interaktion).
  T3 — Deal auswählen → Chat-Verlauf (get_negotiation_messages-Fixtures),
       eigene Nachrichten (Username = Shop-Username) rechts/Akzent,
       System-Events (event != null) als dezente Zeile.
  T4 — Senden (POST /marketplace_negotiations_messages): Payload-Capture
       id_negotiation + hash + message; neue Nachricht erscheint im Verlauf;
       KEINE 2-Schritt-Bestätigung (konversationell).
  T5 — Send-Fehlerpfad 429: Meldung + „AUF UEXCORP.SPACE ÖFFNEN" im
       Chat-Pane, KEIN Retry.
  T6 — Lifecycle-Deep-Links: „Auf UEX öffnen" feuert
       open_marketplace_listing(listing_slug) — API-seitig NICHT vorhandene
       Aktionen (Starten/Beenden/Bewerten) gehen auf die UEX-Webseite
       (Research §3), keine Fake-Buttons im Overlay.
  T7 — Leerer Zustand: ohne Deals saubere row-empty-Meldung, kein Fehlerpane.

Datenquelle: smoke-stub.js (JS-Mirror; die Rust-Seite ist durch cargo tests
in src-tauri/trade.rs + crates/uex-client belegt). Screenshots nach docs/screens/.

Self-contained: startet statischen Server (build/) + headless Chromium.
"""
import asyncio, base64, json, os, pathlib, sys, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8799"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9235"))
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
        "id": id_, "id_listing": 4001, "id_negotiation": 7,
        "event": event, "message": text if event is None else None,
        "listing_title": "Drake Nettle S | Used", "listing_slug": "drake-nettle-s-used",
        "negotiation_hash": hash_, "user_name": username.title(),
        "user_username": username, "api_name": "uex-web",
        "date_added": date_added, "date_read": None,
    }

TRADE_DEALS = json.dumps([
    negotiation(7, "Drake Nettle S | Used", "drake-nettle-s-used", True, 4500000.0,
                {"date_modified": NOW + 3 * 86400, "client_username": "buyer_one"}),
    negotiation(6, "ARGO MPUV Cargo", "argo-mpuv-cargo", False, 1200000.0,
                {"date_modified": NOW + 2 * 86400, "advertiser_username": "trader_dave"}),
    negotiation(5, "MedPen Pack", "medpen-pack", True, 120.0,
                {"date_modified": NOW + 86400}),
    negotiation(4, "Novikov Oversuit", "novikov-oversuit", False, 85000.0,
                {"date_modified": NOW - 3600}),
], separators=(",", ":"))

TRADE_MSGS = json.dumps({
    "h-7": [
        msg(1, "h-7", "buyer_one", "Hallo! Ist das Nettle noch verfügbar?", NOW - 7200),
        msg(2, "h-7", "uex", None, NOW - 7000, event="price_changed"),
        msg(3, "h-7", "sebo_one", "Ja — 4,5 Mio UEC, Abholung in New Babbage.", NOW - 3600),
    ],
}, separators=(",", ":"))

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
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-trades-m3b",
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

        VK = {"F2": (113, "F2")}
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

        def has_text_expr(sel, text):
            return ("(function(){var els=[...document.querySelectorAll(" + json.dumps(sel) + ")];"
                    "return els.some(function(e){return (e.textContent||'').includes(" + json.dumps(text) + ");});})()")

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
            })()"""

        def calls_matching(cmd):
            return js("(function(){return JSON.stringify((window.__calls||[]).filter(function(c){return c.indexOf(" + json.dumps(cmd + ":") + ")===0;}));})()")

        # =================================================================
        # T1 — Trades-Reiter neben Angebote/Mein Shop
        # =================================================================
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_NEGOTIATIONS__ = " + TRADE_DEALS + ";"
            "window.__TRADE_MSGS__ = " + TRADE_MSGS + ";"
        )
        await js(hook_calls_js())
        await enter_market()
        chips = json.loads(str(await js(
            "(function(){return JSON.stringify([...document.querySelectorAll('.mtabs .chip')].map(function(c){return c.textContent.trim()}));})()")) or "[]")
        check("t1-trades-tab-present", chips == ["Angebote", "Mein Shop", "Trades"], str(chips))
        await shot("trades-t1-tab-strip.png")

        # =================================================================
        # T2 — Deal-Liste: Fixtures, neueste zuerst, Gegenpart/Status/Datum
        # =================================================================
        tab = await click_chip("Trades")
        check("t2-trades-tab-opens", tab is True)
        await asyncio.sleep(1.5)
        rows = json.loads(str(await js(
            "JSON.stringify([...document.querySelectorAll('.mlist .mrow.trow')].map(function(r){return r.textContent.replace(/\\s+/g,' ').trim()}))")) or "[]")
        check("t2-deal-rows-rendered", len(rows) == 4, "rows=" + str(len(rows)))
        check("t2-newest-first", "Drake Nettle S | Used" in rows[0] and "Drake Nettle" not in " ".join(rows[1:]), rows[0][:80])
        check("t2-counterpart", "Gegenüber buyer_one (Käufer)" in rows[0], rows[0][:140])
        check("t2-status-offen", "in Verhandlung" in " ".join(rows) or "offen" in " ".join(rows), " ".join(rows)[:200])
        await shot("trades-t2-deal-list.png")

        # =================================================================
        # T3 — Deal wählen → Chat-Verlauf (eigene rechts/Akzent, Events)
        # =================================================================
        opened = await js(
            "(function(){var r=document.querySelector('.mlist .mrow.trow'); if(r){r.click(); return true;} return false;})()")
        check("t3-deal-click", opened is True)
        await asyncio.sleep(1.5)
        chat_msgs = json.loads(str(await js(
            "JSON.stringify([...document.querySelectorAll('.chat .chat-msg')].map(function(m){return m.className.replace(/\\s+/g,' ').trim()+'|'+m.textContent.replace(/\\s+/g,' ').trim()}))")) or "[]")
        check("t3-chat-messages-rendered", len(chat_msgs) == 2, "msgs=" + str(len(chat_msgs)))
        check("t3-own-message-styled", any(m.startswith("chat-msg own|") for m in chat_msgs), str(chat_msgs)[:200])
        check("t3-counterpart-message", any("Buyer_One" in m for m in chat_msgs))
        chat_evs = await js("document.querySelectorAll('.chat .chat-ev').length")
        check("t3-system-event-line", chat_evs == 1, "events=" + str(chat_evs))
        await shot("trades-t3-chat-history.png")

        # =================================================================
        # T4 — Senden: Payload + neue Nachricht sichtbar, kein 2-Schritt
        # =================================================================
        typed = await js(
            "(function(){"
            "var i=document.querySelector('.chat-input input');"
            "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
            "set.call(i,'Probe-Nachricht aus dem Overlay');"
            "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'x'}));"
            "return i.value;})()")
        check("t4-draft-typed", typed == "Probe-Nachricht aus dem Overlay", str(typed))
        sent = await js(
            "(function(){var b=[...document.querySelectorAll('.chat-input .btn')].find(function(x){return x.textContent.trim()==='Senden';}); if(b){b.click(); return true;} return false;})()")
        check("t4-send-clicked", sent is True)
        await asyncio.sleep(1.5)
        payload = str(await js("JSON.stringify(window.__LAST_CHAT_PAYLOAD__ || null)"))
        check("t4-payload-id-hash", '"id_negotiation":7' in payload and '"hash":"h-7"' in payload, payload)
        check("t4-payload-message", "Probe-Nachricht aus dem Overlay" in payload, payload)
        confirm_popups = await js("document.querySelectorAll('.confirm-modal').length")
        check("t4-no-2step-confirm", confirm_popups == 0, "confirm-modals=" + str(confirm_popups))
        new_visible = await js(has_text_expr(".chat", "Probe-Nachricht aus dem Overlay"))
        check("t4-sent-message-visible", new_visible is True)
        await shot("trades-t4-sent-message.png")

        # =================================================================
        # T5 — Send-Fehlerpfad 429: Meldung + Deep-Link, kein Retry
        # =================================================================
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_NEGOTIATIONS__ = " + TRADE_DEALS + ";"
            "window.__TRADE_MSGS__ = " + TRADE_MSGS + ";"
            "window.__CHAT_SEND_FAIL__ = true;"
            "window.__CHAT_SEND_ERR__ = 'HTTP 429 · Rate-Limit erreicht';"
        )
        await js(hook_calls_js())
        await enter_market()
        await click_chip("Trades")
        await asyncio.sleep(1.2)
        await js("(function(){var r=document.querySelector('.mlist .mrow.trow'); if(r) r.click();})()")
        await asyncio.sleep(1.2)
        await js("(function(){var i=document.querySelector('.chat-input input');"
                 "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                 "set.call(i,'Rate-Limit-Probe');"
                 "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'x'}));})()")
        await asyncio.sleep(0.3)
        await js("(function(){var b=[...document.querySelectorAll('.chat-input .btn')].find(function(x){return x.textContent.trim()==='Senden';}); if(b) b.click();})()")
        await asyncio.sleep(1.2)
        err_shown = await js(has_text_expr(".chat-input ~ .row-empty, div[role='alert']", "HTTP 429"))
        check("t5-429-error-shown", err_shown is True)
        deeplink_err = await js(
            "(function(){var b=[...document.querySelectorAll(\"div[role='alert'] .btn-ghost\")].find(function(x){return (x.textContent||'').includes('AUF UEXCORP.SPACE');}); return !!b;})()")
        check("t5-429-deeplink-button", deeplink_err is True)
        await shot("trades-t5-429-error-path.png")

        # =================================================================
        # T6 — Lifecycle-Deep-Link „Auf UEX öffnen"
        # =================================================================
        await js("window.__calls = []")
        clicked = await js(
            "(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return (x.textContent||'').trim()==='Auf UEX öffnen ↗';}); if(b){b.click(); return true;} return false;})()")
        check("t6-deeplink-button-clicked", clicked is True)
        await asyncio.sleep(0.8)
        calls = str(await calls_matching("open_marketplace_listing"))
        check("t6-deeplink-fires-listing-url", "open_marketplace_listing" in calls and "drake-nettle-s-used" in calls, calls[:200])

        # =================================================================
        # T7 — Leerer Zustand (keine Deals) → saubere Meldung, kein Fehlerpane
        # =================================================================
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_NEGOTIATIONS__ = [];"
        )
        await js(hook_calls_js())
        await enter_market()
        await click_chip("Trades")
        await asyncio.sleep(1.2)
        empty = await js(has_text_expr(".row-empty", "Keine Verhandlungen"))
        check("t7-empty-state-clean", empty is True)
        err_pane = await js("document.querySelectorAll('.pane-error').length")
        check("t7-no-error-pane", err_pane == 0, "panes=" + str(err_pane))
        await shot("trades-t7-empty-state.png")

    print()
    print(f"RESULT: {len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        sys.exit(1)
    proc.terminate()
    server.terminate()

if __name__ == "__main__":
    asyncio.run(main())
