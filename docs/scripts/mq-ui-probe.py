#!/usr/bin/env python3
"""CDP-Probe MQ (t_d6ff5212) — Marktplatz-Politur.

Belegt mit funktionalen DOM-Assertions + Screenshots:
  MQ1 — LIGHTBOX: Klick auf Offer-Foto öffnet Inline-Lightbox (kein
        Browser-Deep-Link), Scrim-Klick ODER Esc schließt; externe Links
        bleiben die Buttons (open_marketplace_listing NICHT beim Bildklick).
  MQ2 — FORMULAR-POPUP: größer (>= 760px Breite), Description als
        mehrzeiliges Textfeld (textarea.advert-desc, autogrow, \\n im Wert).
        API-Beleg: docs/research/marketplace-advertise-description.md
        (description string(65535), Newlines erlaubt).
  MQ3 — VERHANDELN: Offer-Detail hat „VERHANDELN AUF UEX“-Button (Deep-Link
        via open_marketplace_listing), Hinweistext im Trades-Reiter,
        „💬 Verhandeln“ pro Listing in Mein Shop.
  MQ4 — SETTINGS: „Nach jeder Aktion (empfohlen)“ / „Nur beim Abmelden/
        Löschen eines Listings“ + 1-Zeilen-Erklärung.

Datenquelle: smoke-stub.js (JS-Mirror der IPC-Commands).
Screenshots nach docs/screens/mq-ui/ (MQ_OUT aus dem Environment).
"""
import asyncio, base64, json, os, pathlib, sys, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("MQ_OUT", str(ROOT / "docs/screens/mq-ui")))
BUILD = pathlib.Path(os.environ.get("MQ_BUILD", str(ROOT / "build")))
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8798"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9232"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

NOW = int(time.time())
def offer(id_, title, location, op, price, qty, extra=None):
    o = {
        "id": id_, "title": title, "description": "",
        "item_id": id_, "category_id": 1, "operation": op,
        "listing_type": "item", "price": price, "currency": "UEC", "unit": "scu",
        "quantity": qty, "location": location, "region": "Stanton",
        "seller": "sebo_one", "quality": 850, "updated": NOW - 3600,
        "expires": None, "slug": f"sebo-{id_}", "photos": [], "approved": True
    }
    o.update(extra or {})
    return o

PHOTO = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
MARKET_POOL = json.dumps([
    offer(701, "SnowBlind (Stealth Suit)", "Area18 — TDD", "sell", 200000, 1,
          {"photos": [PHOTO], "description": "Size: 1 · Grade A"}),
], separators=(",", ":"))
SHOP_OFFERS = json.dumps([
    offer(1, "Gold Ore (Raw)", "Port Tressler", "sell", 4812, 1200),
], separators=(",", ":"))
SHOP_TRADES = json.dumps([], separators=(",", ":"))

# Deep-Link-Zähler: wrappt den Tauri-Stub-invoke (läuft NACH smoke-stub.js).
DL_TRACK = (
    "(function(){"
    "var iv = window.__TAURI_INTERNALS__.invoke;"
    "window.__DL_CALLS__ = {};"
    "window.__TAURI_INTERNALS__.invoke = function(cmd, args){"
    "  window.__DL_CALLS__[cmd] = (window.__DL_CALLS__[cmd]||0)+1;"
    "  return iv.call(window.__TAURI_INTERNALS__, cmd, args);"
    "};"
    "})();"
)

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
        "--directory", str(BUILD),
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
        "--window-size=1920,1500", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-mq-probe",
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

        VK = {"F2": (113, "F2"), "F12": (123, "F12"), "Escape": (27, "Escape")}
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
                "(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()"
            )

        async def enter_market():
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

        # =============================================================
        # MQ1 + MQ3a — Offer-Detail: Lightbox + Verhandeln-Button
        # =============================================================
        await navigate(DL_TRACK + "window.__MARKET_POOL__ = " + MARKET_POOL + ";")
        await enter_market()
        await asyncio.sleep(1.0)
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": "ArrowRight", "code": "ArrowRight", "windowsVirtualKeyCode": 39, "nativeVirtualKeyCode": 39})
        await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": "ArrowRight", "code": "ArrowRight", "windowsVirtualKeyCode": 39, "nativeVirtualKeyCode": 39})
        await asyncio.sleep(0.8)
        detail_open = await js("!!document.querySelector('.mdetail')")
        check("mq1-detail-open", detail_open is True)
        for _ in range(20):
            v = await js("(function(){var im=document.querySelector('.md-photos img'); return im ? im.complete && im.naturalWidth > 0 : false;})()")
            if v is True:
                break
            await asyncio.sleep(0.5)
        photo_loaded = await js("(function(){var im=document.querySelector('.md-photos img'); return im ? im.complete && im.naturalWidth > 0 : false;})()")

        # MQ3a: Verhandeln-Button + Hinweistext im Detail
        neg_btn = await js("(function(){var b=[...document.querySelectorAll('.md-actions .btn')].find(function(x){return x.textContent.includes('VERHANDELN AUF UEX');}); return b ? 'btn' : 'none';})()")
        note = await js("(function(){var n=document.querySelector('.md-note'); return n ? n.textContent : '';})()")
        check("mq3-verhandeln-button", neg_btn == "btn", neg_btn)
        check("mq3-note-text", "Verhandeln startet auf" in str(note) and "uexcorp.space" in str(note))

        # MQ1: Bildklick → Lightbox, KEIN Deep-Link
        dl0 = await js("window.__DL_CALLS__['open_marketplace_listing'] || 0")
        await js("(function(){var b=document.querySelector('.md-photo'); b ? b.click() : null;})()")
        await asyncio.sleep(0.5)
        lb_open = await js("!!document.querySelector('.lightbox-scrim')")
        lb_src = await js("(function(){var im=document.querySelector('.lightbox-modal img'); return im ? im.src.startsWith('data:image/') : false;})()")

        dl1 = await js("window.__DL_CALLS__['open_marketplace_listing'] || 0")
        check("mq1-lightbox-opens", lb_open is True and lb_src is True, "open=%s src=%s" % (lb_open, lb_src))
        check("mq1-no-deeplink-on-photo", photo_loaded is True and dl1 == dl0, "dl before/after=%s/%s photo=%s" % (dl0, dl1, photo_loaded))
        await shot("mq1-lightbox-open.png")

        # Esc schließt
        await key("Escape")
        await asyncio.sleep(0.3)
        lb_closed_esc = await js("!document.querySelector('.lightbox-scrim')")
        check("mq1-esc-closes", lb_closed_esc is True)
        await shot("mq1-lightbox-closed.png")

        # Scrim-Klick schließt (Bildklick bleibt offen)
        await js("(function(){var b=document.querySelector('.md-photo'); b ? b.click() : null;})()")
        await asyncio.sleep(0.4)
        open2 = await js("!!document.querySelector('.lightbox-scrim')")
        await js("(function(){var s=document.querySelector('.lightbox-scrim'); s ? s.click() : null;})()")
        await asyncio.sleep(0.3)
        closed2 = await js("!document.querySelector('.lightbox-scrim')")
        check("mq1-scrim-click-closes", open2 is True and closed2 is True, "open=%s closed=%s" % (open2, closed2))

        # MQ3a: VERHANDELN-Klick ruft Deep-Link mit Listing-Slug
        await js("(function(){var b=[...document.querySelectorAll('.md-actions .btn')].find(function(x){return x.textContent.includes('VERHANDELN AUF UEX');}); b ? b.click() : null;})()")
        await asyncio.sleep(0.4)
        dl2 = await js("window.__DL_CALLS__['open_marketplace_listing'] || 0")
        check("mq3-verhandeln-deeplink", dl2 == dl0 + 1, "dl=%s" % dl2)
        await shot("mq3-offer-detail-negotiate.png")

        # =============================================================
        # MQ3b — Trades-Reiter: Hinweistext (Detail erst schließen)
        # =============================================================
        await key("Escape")
        await asyncio.sleep(0.4)
        detail_gone = await js("!document.querySelector('.mdetail')")
        check("mq3-detail-closed-for-tabs", detail_gone is True)
        tr = await click_chip("Trades")
        await asyncio.sleep(1.2)
        hint = await js("(function(){var els=[...document.querySelectorAll('.row-empty')]; return els.some(function(e){return e.textContent.includes('Deals entstehen auf uexcorp.space');});})()")
        check("mq3-trades-hint", tr is True and hint is True, "chip=%s hint=%s" % (tr, hint))
        await shot("mq3-trades-hint.png")

        # =============================================================
        # MQ2 — Formular-Popup größer + mehrzeilige Description
        #        (frische Navigation: Shop-Fixtures + Secret-Key nötig)
        # =============================================================
        await navigate(
            DL_TRACK +
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
            "window.__SHOP_TRADES__ = " + SHOP_TRADES + ";"
        )
        await enter_market()
        await click_chip("Mein Shop")
        await asyncio.sleep(1.5)
        await click_chip("Anlegen")
        await asyncio.sleep(0.6)
        w = await js("(function(){var m=document.querySelector('.advert-modal'); return m ? Math.round(m.getBoundingClientRect().width) : 0;})()")
        check("mq2-popup-bigger", w >= 760, "width=%spx (vorher 640px)" % w)
        ta = await js("!!document.querySelector('.advert-modal textarea.advert-desc')")
        check("mq2-textarea", ta is True)
        # mehrzeilig setzen (autogrow feuert auf input)
        r = await js("(function(){var t=document.querySelector('.advert-modal textarea.advert-desc');"
                     "if(!t) return 'NO-TA';"
                     "var set=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;"
                     "set.call(t,'Zeile 1: Custodian SMG\\nZeile 2: inkl. 2 Magazine\\nZeile 3: Abholung Port Tressler');"
                     "t.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText'}));"
                     "return JSON.stringify({v:t.value, h:t.getBoundingClientRect().height});})()")
        try:
            rr = json.loads(str(r))
        except Exception:
            rr = {"v": str(r), "h": 0}
        check("mq2-multiline-value", str(rr.get("v", "")).count("\n") == 2, repr(rr.get("v", ""))[:70])
        check("mq2-autogrow", rr.get("h", 0) > 40, "height=%spx" % rr.get("h"))
        # Beleg-Shot: Beschreibungs-Feld in den sichtbaren Bereich rollen
        await js("(function(){var t=document.querySelector('.advert-modal textarea.advert-desc');"
                 "if(t){t.scrollIntoView({block:'center'});"
                 "var p=t.closest('.advert-pane'); if(p){p.scrollTop = t.offsetTop - p.getBoundingClientRect().top - 40;}}})()")
        await asyncio.sleep(0.4)
        await shot("mq2-popup-multiline.png")
        # Popup bleibt in den Overlay-Grenzen
        fits = await js("(function(){var m=document.querySelector('.advert-modal');"
                        "var mr=m.getBoundingClientRect(); var pr=m.parentElement.getBoundingClientRect();"
                        "return mr.top>=pr.top && mr.bottom<=pr.bottom && mr.left>=pr.left && mr.right<=pr.right;})()")
        check("mq2-popup-in-bounds", fits is True)

        # MQ3c: Mein Shop Zeile „💬 Verhandeln“
        await key("Escape")
        await asyncio.sleep(0.3)
        await js("(function(){var b=[...document.querySelectorAll('.advert-modal button')].find(function(x){return x.textContent.trim()==='Verwerfen';}); b ? b.click() : null;})()")
        await asyncio.sleep(0.4)
        shop_neg = await js("(function(){var b=[...document.querySelectorAll('[data-shop-offer-idx] button')].find(function(x){return x.textContent.includes('Verhandeln');}); return b ? b.title : 'none';})()")
        check("mq3-shop-row-button", "uexcorp.space" in str(shop_neg), str(shop_neg))
        await js("(function(){var b=[...document.querySelectorAll('[data-shop-offer-idx] button')].find(function(x){return x.textContent.includes('Verhandeln');}); b ? b.click() : null;})()")
        await asyncio.sleep(0.3)
        dl3 = await js("window.__DL_CALLS__['open_marketplace_listing'] || 0")
        check("mq3-shop-row-deeplink", dl3 == dl2 + 1, "dl=%s" % dl3)

        # =============================================================
        # MQ4 — Settings-Text
        # =============================================================
        await key("F12")
        await asyncio.sleep(1.0)
        cfg_open = await js("(function(){var s=document.body.innerText; return s.includes('Schreibbestätigung');})()")
        opt1 = await js("(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()==='Nach jeder Aktion (empfohlen)';}); return !!b;})()")
        opt2 = await js("(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()==='Nur beim Abmelden/Löschen eines Listings';}); return !!b;})()")
        expl = await js("(function(){var n=[...document.querySelectorAll('.cfg-note')].find(function(x){return x.textContent.includes('Bestätigungspflicht beim Anlegen von Listings');}); return !!n;})()")
        check("mq4-settings-visible", cfg_open is True)
        check("mq4-opt-labels", opt1 is True and opt2 is True, "opt1=%s opt2=%s" % (opt1, opt2))
        check("mq4-explainer", expl is True)
        # Beleg-Shot: Schreibbestätigungs-Block in den sichtbaren Bereich rollen
        await js("(function(){var n=[...document.querySelectorAll('.cfg-note')].find(function(x){return x.textContent.includes('Bestätigungspflicht beim Anlegen von Listings');});"
                 "if(n){n.scrollIntoView({block:'center'});"
                 "var p=n.closest('.pane-detail, .advert-pane, .pane-error') || n.closest('div');"
                 "if(p && p.scrollHeight > p.clientHeight){p.scrollTop = n.offsetTop - 160;}}})()")
        await asyncio.sleep(0.4)
        await shot("mq4-settings-neu.png")

    server.terminate()
    proc.terminate()
    print("\n== MQ-Probe: %d PASS, %d FAIL ==" % (len(PASS), len(FAIL)))
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        sys.exit(1)

asyncio.run(main())
