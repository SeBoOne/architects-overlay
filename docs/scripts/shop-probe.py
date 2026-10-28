#!/usr/bin/env python3
"""CDP-Probe M2a/MB (t_f1543d2a + t_350c4300) — „Mein Shop" + Marktplatz-Nav.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  S0/B1 — Marktplatz: KEIN „Handelsrouten"-Tab im Tabstrip (t_350c4300 B1);
          F6 öffnet Handelsrouten weiterhin (smode HANDELSROUTEN, Rail-Pfad
          nutzt denselben setMode('routes')-Mechanismus).
  S1 — Key-Fehlverhalten (Req #4): ohne gesetzten UEX-API-Key zeigt der
       Mein-Shop-Tab einen sauberen Hinweis + Settings-Link (nicht leer,
       kein Crash); der Link öffnet die Konfiguration.
  S2 — Echte Daten aus Fixtures: eigene Offers + letzte 5 MARKTPLATZ-Trades
       (6 Negotiation-Fixtures → 5, neueste nach date_modified zuerst;
       Listing-Titel statt Commodity-Trace — kein „Agricium" mehr; Status
       abgeschlossen/in Verhandlung, Rolle Verkauf/Kauf); Klick auf eine
       OfferRow feuert open_marketplace_listing(slug), Klick auf eine
       DealRow feuert open_marketplace_listing(listing_slug), Klick auf
       „Listings verwalten" feuert open_marketplace_trades (B3; die URL selbst
       ist per Rust-Unit-Test marketplace_manager_deeplink_is_exact_url
       belegt).
  S3 — Fehlerpfad: beide Reads schlagen fehl → „Mein Shop nicht erreichbar" +
       Fehlermeldung + „AUF UEXCORP.SPACE ÖFFNEN"; danach Offline-Fallback
       (Note + Offline-Snapshot-Chip, Daten bleiben sichtbar).
  S4 — Leere Historie → Leer-Zustand (row-empty), kein Fehlerpane.
  S5 — M2a-Fix (t_2f197d1c): API-Key gesetzt, aber KEIN persönlicher
       Secret-Key → „Secret-Key fehlt oder ist falsch — siehe Einstellungen"
       + Settings-Button, KEIN get_my_offers/get_my_negotiations-Request.

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
PORT = int(os.environ.get("PROBE_PORT", "8796"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9228"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

# ---- M2a/MB-Fixtures (Wire-Feldnamen wie IPC-Serde, snake_case) -------------
NOW = int(time.time())  # echte Jetzt-Zeit — shopStatus vergleicht gegen Date.now()
def offer(id_, title, location, op, price, qty, extra=None):
    o = {
        "id": id_, "title": title, "description": "",
        "item_id": id_, "category_id": 1, "operation": op,
        "listing_type": "item", "price": price, "currency": "UEC", "unit": "scu",
        "quantity": qty, "location": location, "region": "Stanton",
        "seller": "sebo_one", "quality": 850, "updated": NOW - 3600,
        "expires": None, "slug": f"sebo-{id_}"
    }
    o.update(extra or {})
    return o

def negotiation(id_, title, slug, is_advertiser, price, extra=None):
    """marketplace_negotiations-Fixture — Listing-DEAL (t_350c4300 B2), kein
    Commodity-Trade. Neueste Aktivität = date_modified."""
    n = {
        "id": id_, "listing_id": 4000 + id_, "hash": f"h-{id_}",
        "price": price, "unit": "unit", "currency": "UEC",
        "deal_value": None, "deal_value_currency": "",
        "listing_title": title, "listing_slug": slug,
        "advertiser_name": "Semih Balci", "advertiser_username": "sebo_one",
        "client_name": "John Doe", "client_username": "johndoe",
        "is_listing_advertiser": is_advertiser,
        "date_added": NOW - 10 * 86400,          # Verhandlung startete vor 10 Tagen
        "date_modified": NOW + id_ * 86400,      # Aktivität: id_ ↑ = neuer
        "date_closed": None,
    }
    n.update(extra or {})
    return n

SHOP_OFFERS = json.dumps([
    # expires None → Status „aktiv"
    offer(1, "Gold Ore (Raw)", "Port Tressler", "sell", 4812, 1200),
    # expires in Zukunft → „läuft ab …"
    offer(2, "MedPen", "Everus Harbor", "sell", 45, 500, {"expires": NOW + 86400}),
    # expires in Vergangenheit → „abgelaufen"
    offer(3, "Iron Ore (Raw)", "Port Tressler", "buy", 512, 900, {"expires": NOW - 1000}),
], separators=(",", ":"))

SHOP_NEGOTIATIONS = json.dumps([
    # 6 Deals → UI slice(0,5); neueste Aktivität (date_modified) zuerst.
    negotiation(6, "Drake Nettle S | Used", "drake-nettle-s-used", True, 4500000.0,
                {"date_modified": NOW + 5 * 86400, "deal_value": 4400000.0,
                 "deal_value_currency": "UEC", "date_closed": NOW + 5 * 86400,
                 "client_username": "buyer_one"}),
    negotiation(5, "ARGO MPUV Cargo", "argo-mpuv-cargo", False, 1200000.0,
                {"date_modified": NOW + 4 * 86400, "advertiser_username": "trader_dave"}),
    negotiation(4, "MedPen Pack", "medpen-pack", True, 120.0,
                {"date_modified": NOW + 3 * 86400}),
    negotiation(3, "Novikov Oversuit", "novikov-oversuit", False, 85000.0,
                {"date_modified": NOW + 2 * 86400, "advertiser_username": "gear_gal"}),
    negotiation(2, "Ranger CV", "ranger-cv", True, 2750000.0,
                {"date_modified": NOW + 86400}),
    negotiation(1, "Spartan", "spartan", False, 950000.0,
                {"date_modified": NOW, "deal_value": 900000.0,
                 "deal_value_currency": "UEC", "date_closed": NOW}),
], separators=(",", ":"))

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
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-shop-m2a",
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

        async def navigate(stub_extra=""):
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
            if stub_extra:
                await call("Page.addScriptToEvaluateOnNewDocument", {"source": stub_extra})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.2)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            v = r.get("result", {}).get("value")
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return v

        VK = {"F2": (113, "F2"), "F6": (117, "F6")}
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
            data = base64.b64decode(r["data"])
            (OUT / name).write_bytes(data)
            print("  shot:", OUT / name, len(data), "bytes")

        async def click_chip(label):
            return await js(
                "(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()"
            )

        async def click_btn(label):
            return await js(
                "(function(){var b=[...document.querySelectorAll('.btn, .btn-ghost')].find(function(x){return x.textContent.trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()"
            )

        def has_text_expr(sel, text):
            return ("(function(){var els=[...document.querySelectorAll(" + json.dumps(sel) + ")];"
                    "return els.some(function(e){return (e.textContent||'').includes(" + json.dumps(text) + ");});})()")

        async def enter_market():
            # Warmup wie in den bestehenden Proben: ein Such-Pass zwingt die
            # App in den ready-Zustand; Marktplatz wird per F2 betreten.
            await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
            if await js("!!document.querySelector('input.q')"):
                set = ("(function(){"
                       "var i=document.querySelector('input.q');"
                       "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                       "set.call(i,'WARM');"
                       "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'WARM'}));"
                       "return i.value;})()")
                await js(set)
                await asyncio.sleep(0.6)
                await js("(function(){var i=document.querySelector('input.q');"
                         "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                         "set.call(i,'');"
                         "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));})()")
            await js("(function(){var i=document.querySelector('input.q'); if(i){i.focus();}})()")
            await key("F2")
            await asyncio.sleep(1.2)
            smode = await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")
            return str(smode).strip()

        def hook_calls_js():
            return """(function(){
                var orig = window.__TAURI_INTERNALS__.invoke;
                window.__calls = window.__calls || [];
                window.__TAURI_INTERNALS__.invoke = function(cmd,args){
                    window.__calls.push(cmd + ':' + JSON.stringify(args || {}));
                    return orig(cmd,args);
                };
            })()"""

        # =====================================================================
        # S0/B1 — KEIN „Handelsrouten"-Tab im Marktplatz; F6 führt weiter hin
        # =====================================================================
        await navigate()
        await js(hook_calls_js())
        smode = await enter_market()
        check("s0-market-entered", smode == "MARKETPLACE", smode)
        no_routes_tab = await js(
            "(function(){var chips=[...document.querySelectorAll('.mtabs .chip')].map(function(c){return c.textContent.trim()});"
            "return JSON.stringify(chips);})()")
        chips = json.loads(str(no_routes_tab) or "[]")
        check("s0-b1-no-routes-tab", chips == ["Angebote", "Mein Shop"], str(chips))
        # F6 → Handelsrouten trotzdem erreichbar (intern mTab='routes').
        await key("F6")
        await asyncio.sleep(1.0)
        smode_f6 = await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")
        check("s0-b1-f6-opens-routes", str(smode_f6).strip() == "HANDELSROUTEN", str(smode_f6))
        await shot("shop-s0-b1-f6-routes.png")
        # F2 zurück in den Marktplatz (Angebote-Tab).
        await key("F2")
        await asyncio.sleep(0.8)
        tab = await click_chip("Mein Shop")
        check("s1-shop-tab-visible", tab is True, "tab-click=" + str(tab))
        await asyncio.sleep(1.5)
        hint = await js(has_text_expr(".pane-error", "UEX API-Key nicht gesetzt"))
        check("s1-key-hint-shown", hint is True, "pane-error text")
        settings_btn = await js(
            "(function(){var b=[...document.querySelectorAll('.pane-error .btn')].find(function(x){return (x.textContent||'').includes('Einstellungen');}); if(b){b.click(); return true;} return false;})()"
        )
        check("s1-settings-link-clicked", settings_btn is True)
        await asyncio.sleep(0.6)
        cfg_open = await js("document.querySelector('.cfg-title') ? document.querySelector('.cfg-title').textContent.trim() : 'NONE'")
        check("s1-settings-link-opens-config", cfg_open == "KONFIGURATION", cfg_open)
        await shot("shop-s1-no-key-hint.png")

        # =====================================================================
        # S2 — Key gesetzt + Fixtures: Offers + letzte 5 Marktplatz-Trades
        # =====================================================================
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
            "window.__SHOP_NEGOTIATIONS__ = " + SHOP_NEGOTIATIONS + ";"
        )
        await js(hook_calls_js())
        await enter_market()
        await click_chip("Mein Shop")
        await asyncio.sleep(2.0)

        offers_m = await js("[...document.querySelectorAll('[data-shop-offer-idx]')].length")
        check("s2-offers-rendered", offers_m == 3, "rows=" + str(offers_m))
        o1 = await js("JSON.stringify([...document.querySelectorAll('[data-shop-offer-idx]')].map(function(r){return r.textContent.replace(/\\s+/g,' ').trim()}))")
        o1j = json.loads(o1 or "[]")
        check("s2-offer-content", len(o1j) == 3 and all(k in o1j[0] for k in ("Gold Ore (Raw)", "4.812")), str(o1j[0])[:120])
        statuses = " ".join(o1j)
        check("s2-status-aktiv", "aktiv" in statuses)
        check("s2-status-laeuft-ab", "läuft ab" in statuses)
        check("s2-status-abgelaufen", "abgelaufen" in statuses)
        check("s2-offer-terminal", "Port Tressler" in statuses and "Everus Harbor" in statuses)

        heads = await js("[...document.querySelectorAll('.mbar2 .m-lbl')].map(function(x){return x.textContent.trim()})")
        check("s2-trades-heading", any("Letzte 5 Marktplatz-Trades" in h for h in (heads or [])), str(heads))
        # 6 Negotiations im Fixture → genau 5 gerendert (mlist[0]=Offers 3, mlist[1]=Deals 5)
        mlists = json.loads(str(await js("JSON.stringify([...document.querySelectorAll('.mlist')].map(function(l){return l.querySelectorAll('.mrow').length}))") or "[]"))
        check("s2-offers-rows", mlists == [3, 5], "row counts=" + str(mlists))
        check("s2-five-trades", len(mlists) == 2 and mlists[1] == 5, "deal rows=" + str(mlists))
        first_deal = await js(
            "(function(){var l=[...document.querySelectorAll('.mlist')][1]; if(!l) return ''; var r0=l.querySelector('.mrow'); var t=r0.querySelector('.m-title'); var s=r0.querySelector('.m-sub'); var p=r0.querySelector('.m-price'); var mm=r0.querySelectorAll('.m-meta'); var m2=mm[mm.length-1]; return ((t?t.textContent:'')+'|'+(s?s.textContent:'')+'|'+(p?p.textContent:'')+'|'+(m2?m2.textContent:'')).replace(/\\s+/g,' ').trim()})()"
        )
        # Neueste Aktivität (date_modified) zuerst: Drake Nettle S (abgeschlossen)
        check("s2-trades-newest-first", str(first_deal).startswith("Drake Nettle S | Used|"), str(first_deal))
        check("s2-trades-marketplace-content", "Gegenüber buyer_one (Käufer)" in str(first_deal), str(first_deal)[:140])
        check("s2-trades-deal-value", "4.400.000" in str(first_deal), str(first_deal)[:140])
        check("s2-trades-closed-status", "abgeschlossen" in str(first_deal), str(first_deal)[:140])
        # B2-Kern: KEIN Commodity-Trace mehr — weder Agricium noch Cargo-Größen
        all_deals = await js("JSON.stringify([...document.querySelectorAll('.mlist')[1].querySelectorAll('.mrow')].map(function(r){return r.textContent.replace(/\\s+/g,' ').trim()}))")
        all_deals = str(all_deals).lower()
        for relic in ("agricium", "laranite", "/scu", " scU · "):
            check(f"s2-no-commodity-trace-{relic.replace('/','').replace(' ','_')}", relic not in all_deals, relic)
        await js("(function(){var l=[...document.querySelectorAll('.mlist')][1]; if(l) l.scrollTop = l.scrollHeight; var p=document.querySelector('.pane-market'); if(p) p.scrollTop = p.scrollHeight;})()")
        await asyncio.sleep(0.4)
        await shot("shop-s2-offers-and-marketplace-trades.png")

        # Deep-Link B2: DealRow-Klick → open_marketplace_listing(listing_slug)
        await js("(function(){var l=[...document.querySelectorAll('.mlist')][1]; if(!l) return; var r=l.querySelector('.mrow'); if(r) r.click();})()")
        await asyncio.sleep(0.6)
        calls = json.loads(str(await js("JSON.stringify(window.__calls || [])")) or "[]")
        listing_calls = [c for c in calls if "open_marketplace_listing" in c]
        check("s2-deal-deeplink-listing-slug", any("drake-nettle-s-used" in c for c in listing_calls), str(listing_calls[:2]))
        # Deep-Link B3: „Listings verwalten"-Chip → open_marketplace_trades
        mgr_clicked = await click_chip("Listings verwalten ↗")
        check("s2-manager-chip-clicked", mgr_clicked is True)
        await asyncio.sleep(0.6)
        calls2 = json.loads(str(await js("JSON.stringify(window.__calls || [])")) or "[]")
        check("s2-manager-deeplink", any("open_marketplace_trades" in c for c in calls2),
              str([c for c in calls2 if "open_marketplace" in c][-3:]))

        # =====================================================================
        # S3 — Fehler → Meldung + Deep-Link-Button; danach Offline-Snapshot
        # =====================================================================
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "window.__SHOP_OFFERS_FAIL__ = true;"
            "window.__SHOP_NEGOTIATIONS_FAIL__ = true;"
        )
        await js(hook_calls_js())
        await enter_market()
        await click_chip("Mein Shop")
        await asyncio.sleep(2.0)
        err_big = await js(has_text_expr(".pane-error .big", "Mein Shop nicht erreichbar"))
        check("s3-error-pane", err_big is True)
        err_msg = await js("document.querySelector('.pane-error .sm') ? document.querySelector('.pane-error .sm').textContent.trim() : ''")
        check("s3-msg-no-retry-spam", "429" in err_msg or "403" in err_msg, err_msg)
        before = json.loads(str(await js("JSON.stringify(window.__calls || [])")) or "[]")
        await asyncio.sleep(1.5)
        after = json.loads(str(await js("JSON.stringify(window.__calls || [])")) or "[]")
        check("s3-no-auto-retry", len(after) == len(before),
              "invokes before/after=" + str(len(before)) + "/" + str(len(after)))
        deeplink_btn = await js(
            "(function(){var b=[...document.querySelectorAll('.pane-error .btn-ghost')].find(function(x){return (x.textContent||'').includes('AUF UEXCORP');}); if(b){b.click(); return true;} return false;})()"
        )
        check("s3-deeplink-button", deeplink_btn is True)
        await asyncio.sleep(0.5)
        calls3 = json.loads(str(await js("JSON.stringify(window.__calls || [])")) or "[]")
        check("s3-deeplink-fired", any("open_marketplace_trades" in c for c in calls3))
        await shot("shop-s3-error-path.png")

        # Offline-Fallback: Reads liefern wieder, aber als Offline-Snapshot
        await js(
            "window.__SHOP_OFFERS__=" + SHOP_OFFERS + ";"
            "window.__SHOP_NEGOTIATIONS__=" + SHOP_NEGOTIATIONS + ";"
            "window.__SHOP_OFFERS_FAIL__=false; window.__SHOP_NEGOTIATIONS_FAIL__=false;"
            "window.__SHOP_OFFERS_SOURCE__='offline'; window.__SHOP_NEGOTIATIONS_SOURCE__='offline';"
            "window.__SHOP_OFFERS_NOTE__='HTTP 429 · Rate-Limit erreicht';"
            "window.__SHOP_NEGOTIATIONS_NOTE__='HTTP 403 · UEX nicht erreichbar';"
        )
        retried = await click_btn("Erneut versuchen")
        check("s3-retry-clicked", retried is True)
        await asyncio.sleep(1.6)
        offline_chip = await js(has_text_expr(".chip", "Offline-Snapshot"))
        check("s3-offline-chip", offline_chip is True)
        notes = await js("JSON.stringify([...document.querySelectorAll('.row-empty')].map(function(x){return x.textContent.replace(/\\s+/g,' ').trim()}))")
        check("s3-offline-note", "429" in str(notes) or "403" in str(notes), str(notes)[:140])
        still = await js("[...document.querySelectorAll('[data-shop-offer-idx]')].length")
        check("s3-offline-data-survives", still == 3, "offers=" + str(still))
        deals_still = await js(
            "(function(){var l=[...document.querySelectorAll('.mlist')][1]; return l ? l.querySelectorAll('.mrow').length : -1})()")
        check("s3-offline-deals-survive", deals_still == 5, "deals=" + str(deals_still))
        await shot("shop-s3-offline-fallback.png")

        # =====================================================================
        # S4 — leere Historie → LEER-Zustand statt Fehlerpane
        # =====================================================================
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__=[];"
            "window.__SHOP_NEGOTIATIONS__=[];"
            "window.__SHOP_OFFERS_FAIL__=false;"
            "window.__SHOP_NEGOTIATIONS_FAIL__=false;"
        )
        await enter_market()
        await click_chip("Mein Shop")
        await asyncio.sleep(1.8)

        offers_null = await js("[...document.querySelectorAll('[data-shop-offer-idx]')].length")
        check("s4-no-offers-rows", offers_null == 0, "offers=" + str(offers_null))
        trades_empty_state = await js(has_text_expr(".row-empty", "Noch keine Marktplatz-Trades"))
        check("s4-empty-history-empty-state", trades_empty_state is True)
        err_pane_s4 = await js(has_text_expr(".pane-error .big", "Mein Shop nicht erreichbar"))
        check("s4-no-error-pane-on-empty", err_pane_s4 is False)
        offers_empty = await js(has_text_expr(".row-empty", "Keine eigenen Offers"))
        check("s4-empty-offers-state", offers_empty is True)
        await shot("shop-s4-empty-history.png")

        # =====================================================================
        # S5 — API-Key gesetzt, KEIN Secret-Key → Hinweis, KEIN Request
        # =====================================================================
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = false;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__=" + SHOP_OFFERS + ";"
            "window.__SHOP_NEGOTIATIONS__=" + SHOP_NEGOTIATIONS + ";"
            "window.__SHOP_OFFERS_FAIL__=false;"
            "window.__SHOP_NEGOTIATIONS_FAIL__=false;"
        )
        await js(hook_calls_js())
        await enter_market()
        await click_chip("Mein Shop")
        await asyncio.sleep(1.5)
        sk_hint = await js(has_text_expr(".pane-error .big", "Secret-Key fehlt oder ist falsch"))
        check("s5-secret-key-hint-shown", sk_hint is True)
        sk_calls = json.loads(str(await js("JSON.stringify(window.__calls || [])")) or "[]")
        shop_reads = [c for c in sk_calls if "get_my_offers" in c or "get_my_negotiations" in c]
        check("s5-no-shop-request-without-secret-key", len(shop_reads) == 0, str(shop_reads[:3]))
        await shot("shop-s5-secret-key-missing.png")
        sk_btn = await js(
            "(function(){var b=[...document.querySelectorAll('.pane-error .btn')].find(function(x){return (x.textContent||'').includes('Einstellungen');}); if(b){b.click(); return true;} return false;})()"
        )
        check("s5-secret-key-settings-link-clicked", sk_btn is True)
        await asyncio.sleep(0.6)
        await js("(function(){var p=document.querySelector('.pane-config'); if(p) p.scrollTop = p.scrollHeight;})()")
        await asyncio.sleep(0.5)
        await shot("shop-s5-settings-secret-key-field.png")

        await call("Page.close")

    proc.terminate()
    server.terminate()

    print("\n===== RESULT =====")
    print("PASS:", len(PASS), "FAIL:", len(FAIL))
    for f in FAIL:
        print("  FAILED:", f)
    sys.exit(1 if FAIL else 0)

if __name__ == "__main__":
    asyncio.run(main())
