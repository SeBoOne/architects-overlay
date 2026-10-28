#!/usr/bin/env python3
"""CDP-Probe M3a (t_4fee0d19) — Formular-Popup, Offer-Bilder, pending Listings.

Funktionale DOM-Assertions auf feature/mkt-shop-ui-v2:
  A — „Neues Listing“ als modales Popup: Scrim + feste Karte, Abbrechen mit
      ungespeicherten Eingaben → „Eingaben verwerfen?“ (Weiter/Verwerfen).
  B — Angebots-Detail zeigt `photos` (Thumbnail-Kacheln); ohne Bilder
      sauberer Leerzustand („Kein Bild hinterlegt.“).
  C — Mein Shop: Wartend-Chip + Filter Alle/Aktiv/Wartend (clientseitig,
      date_approved=0 ⇒ wartet auf Freigabe).

Datenquelle: smoke-stub.js (IPC-Grenze gestubbt; die Rust-Seite ist durch
cargo tests in crates/uex-client (listing_row_photos_and_approval_m3a) +
crates/cache belegt). Screenshots nach docs/screens/m3a-*.png.

Self-contained: startet statischen Server (build/) + headless Chromium.
"""
import asyncio, base64, json, os, pathlib, sys, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8799"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9231"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

NOW = int(time.time())
# Hermetisches Probe-Bild: zuvor live CDN-URL (cdn.uexcorp.space ...-t.jpg) —
# im Headless-Chromium nicht deterministisch ladbar (Netz-Flake), Render-Pfad
# identisch: URL aus `photos` wird unverändert als <img src> gesetzt.
PHOTO = f"http://127.0.0.1:{os.environ.get('PROBE_PORT', '8799')}/m3a-probe-photo.png"


def write_probe_photo() -> None:
    import struct, zlib
    w = h = 64
    raw = b"".join(b"\x00" + bytes([240, 120, 40] * w) for _ in range(h))

    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))

    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw))
           + chunk(b"IEND", b""))
    (ROOT / "build" / "m3a-probe-photo.png").write_bytes(png)


write_probe_photo()

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

# B: Markt-Pool mit einem Bild-Listing
MARKET_POOL = json.dumps([
    offer(701, "SnowBlind (Stealth Suit)", "Area18 — TDD", "sell", 200000, 1,
          {"photos": [PHOTO], "description": "Size: 1 · Grade A"}),
    offer(702, "Ohne-Bild-Listing", "Port Tressler", "sell", 100, 5, {"description": "test"}),
], separators=(",", ":"))

# C: Mein-Shop-Fixtures — 2 aktiv + 1 wartend
SHOP_OFFERS = json.dumps([
    offer(1, "Gold Ore (Raw)", "Port Tressler", "sell", 4812, 1200),
    offer(2, "MedPen", "Everus Harbor", "sell", 45, 500),
    offer(3, "Titanium (Neu)", "Lorville", "sell", 3150, 200,
          {"approved": False, "description": "wartet auf Freigabe"}),
], separators=(",", ":"))

SHOP_TRADES = json.dumps([], separators=(",", ":"))

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
        "--window-size=1920,1500", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-m3a-probe",
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
                "(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()"
            )

        async def click_btn(label):
            return await js(
                "(function(){var b=[...document.querySelectorAll('.btn, .btn-ghost')].find(function(x){return x.textContent.trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()"
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
            return str(await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")).strip()

        def rows_expr(sel):
            return "JSON.stringify([...document.querySelectorAll(" + json.dumps(sel) + ")].map(function(r){return r.textContent.replace(/\\s+/g,' ').trim()}))"

        # =================================================================
        # A — Formular-Popup (Mein Shop → Anlegen)
        # =================================================================
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
            "window.__SHOP_TRADES__ = " + SHOP_TRADES + ";"
        )
        await enter_market()
        await click_chip("Mein Shop")
        await asyncio.sleep(1.5)

        open_clicked = await click_chip("Anlegen")
        check("a-anlegen-opens", open_clicked is True)
        await asyncio.sleep(0.6)
        modal = await js("!!document.querySelector('.advert-modal')")
        scrim = await js("!!document.querySelector('.advert-scrim')")
        check("a-modal-renders", modal is True and scrim is True, "modal=%s scrim=%s" % (modal, scrim))
        head = await js("(function(){var m=document.querySelector('.advert-modal'); return m ? m.textContent.includes('NEUES LISTING ANLEGEN') : false;})()")
        check("a-modal-headline", head is True)
        # Buttons liegen in der Karte (immer erreichbar, keine Abbrüche)
        btn_in = await js("(function(){var m=document.querySelector('.advert-modal'); return m ? !!m.querySelector('.btn') : false;})()")
        check("a-buttons-in-modal", btn_in is True)
        # Buttons immer erreichbar: Anlegen/Abbrechen komplett im Viewport
        # (sticky Footer in der Scroll-Region), nicht unter dem Falz.
        vis = await js(
            "(function(){var m=document.querySelector('.advert-modal');"
            "var mr=m.getBoundingClientRect(); var pr=m.parentElement.getBoundingClientRect();"
            "var clipB=Math.min(mr.bottom,pr.bottom), clipT=Math.max(mr.top,pr.top);"
            "var names=['Anlegen (2-Schritt)','Abbrechen'];"
            "return JSON.stringify(names.map(function(n){"
            "var b=[...m.querySelectorAll('button')].find(function(x){return x.textContent.trim()===n;});"
            "if(!b) return false; var r=b.getBoundingClientRect();"
            "return r.height>0 && r.top>=clipT && r.bottom<=clipB && r.bottom<=window.innerHeight;}));})()"
        )
        vis_list = json.loads(str(vis) or "[]")
        check("a-buttons-in-viewport", vis_list == [True, True], str(vis_list))
        if os.environ.get("PROBE_DEBUG"):
            geo = await js(
                "(function(){function r(e){var b=e.getBoundingClientRect();"
                "return [Math.round(b.top),Math.round(b.bottom)];}"
                "var m=document.querySelector('.advert-modal');"
                "var s=m.parentElement; var foot=m.querySelector('.advert-foot');"
                "var pane=m.querySelector('.advert-pane');"
                "return JSON.stringify({modal:r(m),scrim:r(s),foot:foot?r(foot):null,"
                "pane:pane?r(pane):null,paneScrollTop:pane?pane.scrollTop:null,"
                "innerH:window.innerHeight,"
                "cs_maxh:getComputedStyle(m).maxHeight,"
                "cs_pos:getComputedStyle(s).position});})()"
            )
            print("  GEO:", geo)
        await shot("m3a-a-popup-open.png")

        # Eingabe machen → Abbrechen → Verwerfen-Frage
        await js("(function(){var m=document.querySelector('.advert-modal'); var i=m.querySelector('input[placeholder*=Artikelname]'); "
                 "if(!i) return 'NO-INPUT';"
                 "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set; "
                 "set.call(i,'Test-Listing'); i.dispatchEvent(new InputEvent('input',{bubbles:true})); return 'OK';})()")
        await asyncio.sleep(0.3)
        cancel = await click_btn("Abbrechen")
        await asyncio.sleep(0.4)
        discard = await js("(function(){var d=document.querySelector('.advert-discard'); return d ? d.textContent.includes('Eingaben verwerfen?') : false;})()")
        check("a-discard-question", cancel is True and discard is True, "discard=" + str(discard))
        await shot("m3a-a-discard-confirm.png")
        # Weiter bearbeiten → Popup bleibt
        await click_btn("Weiter bearbeiten")
        await asyncio.sleep(0.3)
        still = await js("!!document.querySelector('.advert-modal')")
        gone_discard = await js("!document.querySelector('.advert-discard')")
        check("a-continue-keeps-modal", still is True and gone_discard is True)
        # Abbrechen → Verwerfen → zu
        await click_btn("Abbrechen")
        await asyncio.sleep(0.3)
        await click_btn("Verwerfen")
        await asyncio.sleep(0.4)
        closed = await js("!document.querySelector('.advert-scrim')")
        check("a-discard-closes", closed is True)
        # Ohne Eingaben: Abbrechen schließt direkt (keine Frage)
        await click_chip("Anlegen")
        await asyncio.sleep(0.4)
        await click_btn("Abbrechen")
        await asyncio.sleep(0.4)
        closed2 = await js("!document.querySelector('.advert-scrim') && !document.querySelector('.advert-discard')")
        check("a-clean-cancel-closes-direct", closed2 is True)
        await shot("m3a-a-popup-closed.png")

        # =================================================================
        # B — Angebots-Detail mit Bild / Leerzustand
        # =================================================================
        await navigate(
            "window.__MARKET_POOL__ = " + MARKET_POOL + ";"
        )
        await enter_market()
        await asyncio.sleep(1.0)
        # Angebotsliste: erstes Angebot (SnowBlind, mit Bild) öffnen (Pfeil rechts)
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        VK2 = {"ArrowRight": (39, "ArrowRight"), "Escape": (27, "Escape")}
        for k in ["ArrowRight"]:
            vk, code = VK2[k]
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.3)
        await asyncio.sleep(0.8)
        detail_open = await js("!!document.querySelector('.mdetail')")
        photos = await js("[...document.querySelectorAll('.md-photos .md-photo img')].length")
        photo_loaded = False
        for _ in range(20):
            v = await js("(function(){var im=document.querySelector('.md-photos img'); return im ? im.complete && im.naturalWidth > 0 : false;})()")
            if v is True:
                photo_loaded = True
                break
            await asyncio.sleep(0.5)
        check("b-detail-photos-rendered", detail_open is True and photos == 1 and photo_loaded is True,
              "detail=%s photos=%s loaded=%s" % (detail_open, photos, photo_loaded))
        await shot("m3a-b-detail-photo.png")
        # Leerzustand: Detail ohne Bild
        for k in ["Escape", "ArrowDown", "ArrowRight"]:
            vk, code = VK2.get(k, (0, k))
            if k == "Escape":
                await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": "Escape", "windowsVirtualKeyCode": 27, "nativeVirtualKeyCode": 27})
                await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": "Escape", "windowsVirtualKeyCode": 27, "nativeVirtualKeyCode": 27})
            else:
                await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
                await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.3)
        await asyncio.sleep(0.8)
        empty_state = await js("(function(){var e=document.querySelector('.md-desc.empty'); return e ? e.textContent.includes('Kein Bild') : false;})()")
        no_photos = await js("document.querySelectorAll('.md-photos').length")
        check("b-empty-photo-state", empty_state is True and no_photos == 0, "empty=%s photo-rows=%s" % (empty_state, no_photos))
        await shot("m3a-b-detail-empty.png")

        # =================================================================
        # C — Mein Shop: Wartend-Chip + Filter Alle/Aktiv/Wartend
        # =================================================================
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
            "window.__SHOP_TRADES__ = " + SHOP_TRADES + ";"
        )
        await enter_market()
        await click_chip("Mein Shop")
        await asyncio.sleep(1.5)

        chips = await js("JSON.stringify([...document.querySelectorAll('.mbar2 .chip')].map(function(c){return c.textContent.trim()}))")
        chip_list = json.loads(str(chips) or "[]")
        check("c-filter-chips", all(x in str(chip_list) for x in ("Alle", "Aktiv", "Wartend")), str(chip_list))
        rows = json.loads(str(await js(rows_expr("[data-shop-offer-idx]"))) or "[]")
        check("c-all-shows-3", len(rows) == 3, "rows=" + str(len(rows)))
        pending_chip = await js("(function(){var p=document.querySelector('[data-shop-offer-idx] .chip.warn'); return p ? p.textContent.includes('wartet auf Freigabe') : false;})()")
        pending_row = any("Titanium (Neu)" in r and "wartet auf Freigabe" in r for r in rows)
        check("c-pending-chip-on-row", pending_chip is True and pending_row is True, str(pending_chip))
        await shot("m3a-c-pending-chip.png")

        await click_chip("Wartend")
        await asyncio.sleep(0.5)
        rows_p = json.loads(str(await js(rows_expr("[data-shop-offer-idx]"))) or "[]")
        check("c-wartend-filter", len(rows_p) == 1 and "Titanium (Neu)" in rows_p[0], str(rows_p))
        await shot("m3a-c-wartend-filter.png")

        await click_chip("Aktiv")
        await asyncio.sleep(0.5)
        rows_a = json.loads(str(await js(rows_expr("[data-shop-offer-idx]"))) or "[]")
        check("c-aktiv-filter", len(rows_a) == 2 and all("Titanium (Neu)" not in r for r in rows_a), str(rows_a))

        await click_chip("Alle")
        await asyncio.sleep(0.5)
        rows_all = json.loads(str(await js(rows_expr("[data-shop-offer-idx]"))) or "[]")
        check("c-alle-filter", len(rows_all) == 3, str(len(rows_all)))

    server.terminate()
    proc.terminate()
    print("\n== M3a-Probe: %d PASS, %d FAIL ==" % (len(PASS), len(FAIL)))
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        sys.exit(1)

asyncio.run(main())
