#!/usr/bin/env python3
"""CDP-Probe M2b (t_6e26ec82) — „Mein Shop" Write-Layer auf feature/marketplace-mgr.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  W1 — ANLEGEN (2-Schritt, Default every_action): Formular öffnen, Pflichtangaben,
       ers�ch den 2-Schritt-Confirm „Listing ANLEGEN?" → JA → `marketplace_advertise`
       feuert → PULL refehlt die lokale Offers-Liste (neues Listing sichtbar).
  W2 — ABMELDEN (2-Schritt immer): Klick auf Abmelden → Confirm „Listing ABMELDEN?"
       → JA → `marketplace_delete` feuert → Offer verschwindet (Pull).
  W3 — CONFIRM-MODE „delete_only": Anlegen läuft DIREKT (kein Confirm-Dialog),
       Abmelden bestätigt weiterhin (2-Schritt bleibt fürs Löschen).
  W4 — FEHLERPfad: Write-Fail (HTTP 429/403) → Fehlermeldung + Deep-Link-Button,
       KEIN Auto-Retry (invoke-Zähler wächst nicht).

Datenquelle: smoke-stub.js (JS-Mirror der IPC-Commands; die Rust-Seite ist durch
cargo tests in src-tauri/trade.rs + crates/uex-client belegt). Screenshots nach
docs/screens/. Self-contained: statischer Server (build/) + headless Chromium.
"""
import asyncio, base64, json, os, pathlib, sys, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8796"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9229"))
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
        "expires": None, "slug": f"sebo-{id_}"
    }
    o.update(extra or {})
    return o

SHOP_OFFERS = json.dumps([
    offer(1, "Gold Ore (Raw)", "Port Tressler", "sell", 4812, 1200),
    offer(2, "MedPen", "Everus Harbor", "sell", 45, 500),
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
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-shop-m2b",
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
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1600, "deviceScaleFactor": 2, "mobile": False})

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
                "(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return (x.textContent||'').trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()"
            )
        async def click_btn(label):
            return await js(
                "(function(){var b=[...document.querySelectorAll('.btn, .btn-ghost')].find(function(x){return (x.textContent||'').trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()"
            )
        def has_text(sel, text):
            return ("(function(){var els=[...document.querySelectorAll(" + json.dumps(sel) + ")];"
                    "return els.some(function(e){return (e.textContent||'').includes(" + json.dumps(text) + ");});})()")

        async def enter_shop():
            # Beim Shop-Tab laden wir NICHT über Marketplace-Enter — wir klicken
            # direkt auf den Mein-Shop-Tab (Shop braucht keinen Markt-Sweep).
            await js("(function(){if(document.querySelector('.smode')) })()")
            await js("(function(){var i=document.querySelector('input.q'); if(i){i.focus();}})()")
            await key("F2")
            await asyncio.sleep(1.0)
            await click_chip("Mein Shop")
            await asyncio.sleep(1.5)

        def invoke_watch():
            return js("""(function(){
                var orig = window.__TAURI_INTERNALS__.invoke;
                window.__calls = window.__calls || [];
                window.__TAURI_INTERNALS__.invoke = function(cmd,args){
                    window.__calls.push(cmd + ':' + JSON.stringify(args || {}));
                    return orig(cmd,args);
                };
            })()""")

        # ---------------------------------------------------------------
        # W1 — ANLEGEN (every_action): Formular → 2-Schritt → JA →
        #      marketplace_advertise → Pull zeigt das neue Listing.
        # ---------------------------------------------------------------
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
            "window.__stubState.marketplace_confirm = 'every_action';"
        )
        await invoke_watch()
        await enter_shop()

        # Anlegen-Button in der Offers-Header-Leiste.
        anlegen = await click_chip("Anlegen")
        check("w1-anlegen-button", anlegen is True)
        await asyncio.sleep(0.5)
        form_open = await js("document.body.innerText.includes('NEUES LISTING ANLEGEN')")
        check("w1-form-opened", form_open is True)

        # Pflichtangaben füllen (Titel + Beschreibung; Preis/Menge explizit —
        # emptyAdvertise startet price:'' und die App lehnt leeren Preis korrekt ab).
        # M2b-Review-Fix: Kategorie ist Pflicht (id_category) — im <select>
        # die erste angebotene Kategorie wählen.
        fill = ("(function(){"
                "function setVal(sel,v){var i=typeof sel==='string'?document.querySelector(sel):sel;"
                "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                "set.call(i,v);i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:v}));}"
                "setVal('input[placeholder*=Artikelname]','DRAKE NETTLE S | Test');"
                "setVal('input[placeholder*=Freitext]','Minimales M2b-Testlisting.');"
                "setVal(document.querySelectorAll('input.qnum')[0],'120000');"
                "var cat=document.querySelector('select[aria-label=Kategorie]');"
                "if(cat){cat.value=cat.options[1].value;cat.dispatchEvent(new Event('change',{bubbles:true}));}"
                "return true;})()")
        await js(fill)
        await asyncio.sleep(0.3)

        # „Anlegen (2-Schritt)" → es erscheint der CONFIRM-Dialog (every_action).
        submit = await click_btn("Anlegen (2-Schritt)")
        check("w1-submit-clicked", submit is True)
        await asyncio.sleep(0.5)
        confirm_big = await js("document.body.innerText.includes('Listing ANLEGEN?')")
        check("w1-confirm-dialog", confirm_big is True, "every_action => 2-Schritt")
        await shot("shop-w1-every-action-confirm.png")

        # Noch NICHT gesendet (Confirm hat den Write noch nicht gefeuert).
        before_confirm = json.loads(str(await js("JSON.stringify(window.__calls||[])")) or "[]")
        pre_writes = [c for c in before_confirm if "marketplace_advertise" in c]
        check("w1-no-write-before-confirm", len(pre_writes) == 0, "invokes=" + str(len(before_confirm)))

        # JA, ANLEGEN → Write feuert.
        ja = await click_btn("JA, ANLEGEN")
        check("w1-confirm-yes", ja is True)
        await asyncio.sleep(1.2)  # Pull der Offers-Liste
        after = json.loads(str(await js("JSON.stringify(window.__calls||[])")) or "[]")
        adv = [c for c in after if "marketplace_advertise" in c]
        check("w1-marketplace_advertise-fired", len(adv) >= 1, str(adv[:1]))
        # M2b-Review-Fix: der Payload trägt id_category (Zahl, nicht null) —
        # ohne sie lehnt UEX den Item-Create mit 400 missing_id_category ab.
        payload_cat = None
        for c in after:
            if "marketplace_advertise" in c:
                try:
                    args = json.loads(c.split(":", 1)[1])
                    payload_cat = args.get("payload", {}).get("id_category")
                except Exception:
                    pass
        check("w1-payload-carries-id_category", isinstance(payload_cat, int) and payload_cat > 0, "id_category=" + str(payload_cat))
        # M2b-Review-Fix (t_6e26ec82 R2): Erfolgsmeldung + Deep-Link hängen an
        # der Create-Antwort `{ id, url }` (NICHT am Pull — frische Listings
        # erscheinen erst nach UEX-Approval). PULL: lokale Liste neu laden.
        success_note = await js("document.body.innerText.includes('wurde angelegt.')")
        check("w1-success-note", success_note is True)
        success_link = await js("(function(){var b=[...document.querySelectorAll('button, .btn-ghost')].find(function(x){return (x.textContent||'').includes('ANGELEGTES LISTING');}); return !!b;})()")
        check("w1-success-deeplink", success_link is True, "Deep-Link aus Create-Response")
        # PULL: neues Listing ist in der Offers-Liste sichtbar.
        title_shown = await js("document.body.innerText.includes('DRAKE NETTLE S | Test')")
        check("w1-pull-shows-new-offer", title_shown is True)
        offers = await js("[...document.querySelectorAll('[data-shop-offer-idx]')].length")
        check("w1-offers-count-grew", int(offers or 0) == 3, "rows=" + str(offers))
        await shot("shop-w1-after-affiliate-pull.png")

        # ---------------------------------------------------------------
        # W2 — ABMELDEN (2-Schritt immer): Abmelden → Confirm → delete.
        # ---------------------------------------------------------------
        # Wir gehen einen frischen Navigate, damit der Pull nicht die eben
        # angelegte Fixture überschreibt — Abmelden läuft gegen die initialen 2.
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
            "window.__stubState.marketplace_confirm = 'every_action';"
        )
        await invoke_watch()
        await enter_shop()
        did = await js("document.body.innerText.includes('Gold Ore (Raw)')")
        check("w2-offer-present", did is True)

        ab = await js("(function(){var b=[...document.querySelectorAll('button')].find(function(x){return (x.textContent||'').trim()==='Abmelden';}); if(b){b.click(); return true;} return false;})()")
        check("w2-abmelden-clicked", ab is True)
        await asyncio.sleep(0.5)
        del_conf = await js("document.body.innerText.includes('Listing ABMELDEN?')")
        check("w2-delete-confirm-dialog", del_conf is True)
        ja_ab = await click_btn("JA, ABMELDEN")
        check("w2-confirm-yes", ja_ab is True)
        await asyncio.sleep(1.2)
        after2 = json.loads(str(await js("JSON.stringify(window.__calls||[])")) or "[]")
        dels = [c for c in after2 if "marketplace_delete" in c]
        check("w2-marketplace_delete-fired", len(dels) >= 1, str(dels[:1]))
        gold_gone = await js("(function(){var titles=[...document.querySelectorAll('.mrow .m-title')].map(function(x){return (x.textContent||'').trim();});return titles.length===1 && !titles.some(function(t){return t.includes('Gold Ore');});})()")
        check("w2-pull-removes-offer", gold_gone is True, "Verbleibende mrow-Titel: nur noch MedPen (Erfolgs-Note enthält den Titel absichtlich)")
        await shot("shop-w2-after-delete-pull.png")

        # ---------------------------------------------------------------
        # W3 — Confirm-Mode „delete_only": Anlegen DIREKT, Abmelden bestätigt.
        # ---------------------------------------------------------------
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
            "window.__stubState.marketplace_confirm = 'delete_only';"
        )
        await invoke_watch()
        await enter_shop()
        await click_chip("Anlegen")
        await asyncio.sleep(0.4)
        await js("(function(){function setVal(sel,v){var i=typeof sel==='string'?document.querySelector(sel):sel;var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;set.call(i,v);i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:v}));}setVal('input[placeholder*=Artikelname]','Desperation A');setVal('input[placeholder*=Freitext]','Delete-only Test.');setVal(document.querySelectorAll('input.qnum')[0],'45000');var cat=document.querySelector('select[aria-label=Kategorie]');if(cat){cat.value=cat.options[1].value;cat.dispatchEvent(new Event('change',{bubbles:true}));}return true;})()")
        await asyncio.sleep(0.3)
        await click_btn("Anlegen (2-Schritt)")
        await asyncio.sleep(0.6)
        no_confirm = await js("!document.body.innerText.includes('Listing ANLEGEN?')")
        check("w3-delete_only-no-advertise-confirm", no_confirm is True, "Anlegen läuft direkt")
        after3 = json.loads(str(await js("JSON.stringify(window.__calls||[])")) or "[]")
        adv3 = [c for c in after3 if "marketplace_advertise" in c]
        check("w3-advertise-fired-direct", len(adv3) >= 1, str(adv3[:1]))
        # Abmelden bestätigt weiterhin:
        await js("(function(){var b=[...document.querySelectorAll('button')].find(function(x){return (x.textContent||'').trim()==='Abmelden';}); if(b){b.click(); return true;} return false;})()")
        await asyncio.sleep(0.5)
        del_conf3 = await js("document.body.innerText.includes('Listing ABMELDEN?')")
        check("w3-delete-still-confirms", del_conf3 is True, "Löschen bleibt 2-Schritt")
        await shot("shop-w3-delete-only-mode.png")

        # ---------------------------------------------------------------
        # W4 — FEHLERPfad: Write schlägt fehl → Meldung + Deep-Link, kein Retry.
        # ---------------------------------------------------------------
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
            "window.__SHOP_WRITE_FAIL__ = true;"
            "window.__SHOP_WRITE_ERR__ = 'HTTP 429 · Rate-Limit erreicht';"
        )
        await invoke_watch()
        await enter_shop()
        # Abmelden → Confirm → der eigentliche Write schlägt fehl.
        await js("(function(){var b=[...document.querySelectorAll('button')].find(function(x){return (x.textContent||'').trim()==='Abmelden';}); if(b){b.click(); return true;} return false;})()")
        await asyncio.sleep(0.5)
        await click_btn("JA, ABMELDEN")
        await asyncio.sleep(1.0)
        err_shown = await js("document.body.innerText.includes('HTTP 429')")
        check("w4-error-message-shown", err_shown is True)
        # Deep-Link-Button (passende UEX-Seite) ist da.
        deeplink = await js("(function(){var b=[...document.querySelectorAll('button, .btn-ghost')].find(function(x){return (x.textContent||'').includes('AUF UEXCORP');}); return !!b;})()")
        check("w4-deeplink-button", deeplink is True)
        # Kein Auto-Retry: der Write-Fehler darf NICHT neue invokes auslösen.
        b4 = json.loads(str(await js("JSON.stringify(window.__calls||[])")) or "[]")
        await asyncio.sleep(1.5)
        a4 = json.loads(str(await js("JSON.stringify(window.__calls||[])")) or "[]")
        check("w4-no-auto-retry", len(a4) == len(b4), f"invokes before/after={len(b4)}/{len(a4)}")
        await shot("shop-w4-error-path.png")

        # ---------------------------------------------------------------
        # W5 — KATEGORIE-PFLICHT (M2b-Review-Fix t_6e26ec82): Formular ohne
        #      Kategorie-Auswahl → deutsche Meldung + KEIN Write (UEX würde
        #      400 missing_id_category senden — die UI verhindert den POST).
        # ---------------------------------------------------------------
        await navigate(
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
            "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
            "window.__stubState.marketplace_confirm = 'every_action';"
        )
        await invoke_watch()
        await enter_shop()
        await click_chip("Anlegen")
        await asyncio.sleep(0.6)
        # Kategorien-Dropdown ist geladen (get_categories-Stub).
        cats = await js("[...document.querySelectorAll('select[aria-label=Kategorie] option')].map(function(o){return o.value;})")
        check("w5-category-dropdown-loaded", isinstance(cats, list) and len(cats) >= 2, "options=" + str(cats))
        # Formular OHNE Kategorie füllen (Default '' = bitte wählen).
        await js("(function(){function setVal(sel,v){var i=typeof sel==='string'?document.querySelector(sel):sel;var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;set.call(i,v);i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:v}));}setVal('input[placeholder*=Artikelname]','Keine Kategorie');setVal('input[placeholder*=Freitext]','Soll ohne Kategorie scheitern.');setVal(document.querySelectorAll('input.qnum')[0],'5000');return true;})()")
        await asyncio.sleep(0.3)
        await click_btn("Anlegen (2-Schritt)")
        await asyncio.sleep(0.5)
        # Confirm-Dialog darf NICHT erscheinen; stattdessen deutsche Meldung.
        no_confirm5 = await js("!document.body.innerText.includes('Listing ANLEGEN?')")
        check("w5-no-confirm-without-category", no_confirm5 is True)
        # Die Meldung rendert als .chip.warn im Formular (innerText verdeckt
        # sie wegen overflow-clipped Container — DOM-Assertion auf das Element).
        msg5 = await js("(function(){var c=[...document.querySelectorAll('.chip.warn')].find(function(x){return (x.textContent||'').includes('Bitte eine Kategorie wählen');}); return !!c;})()")
        check("w5-german-category-message", msg5 is True)
        after5 = json.loads(str(await js("JSON.stringify(window.__calls||[])")) or "[]")
        adv5 = [c for c in after5 if "marketplace_advertise" in c]
        check("w5-no-write-without-category", len(adv5) == 0, "kein marketplace_advertise")
        await shot("shop-w5-missing-category-blocked.png")

    # Cleanup
    proc.terminate()
    await proc.wait()
    server.terminate()
    await server.wait()

    print("\n===== M2b Write-Layer Probe =====")
    print(f"PASS {len(PASS)} / FAIL {len(FAIL)}")
    for f in FAIL:
        print("  FAILED:", f)
    sys.exit(1 if FAIL else 0)

if __name__ == "__main__":
    asyncio.run(main())