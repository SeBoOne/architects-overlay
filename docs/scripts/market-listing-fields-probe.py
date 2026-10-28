#!/usr/bin/env python3
"""CDP-Probe MC (t_5faaf946) — Listing-Formular: fehlende Felder + Autocomplete + Bilder.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  MC1 — Formular zeigt ALLE UEX-Felder: Listing Type, Item, Currency,
        Item Source, Quality, Hide Listing, Availability + Bilder-Input.
  MC2 — ORT-AUTOCOMPLETE: location_suggest liefert nur Locations (keine
        Shops); Typeahead öffnet beim Tippen, Enter übernimmt den Ort.
  MC3 — ITEM-TYPEAHEAD: item_suggest liefert Items; Auswahl setzt item_id;
        Item-Feld nur bei Listing Type=Item sichtbar.
  MC4 — PFLICHT-VALIDIERUNG: leeres Formular → deutsche Meldungen pro Feld
        (Titel/Kategorie/Item/Ort/Preis/Menge), KEIN marketplace_advertise.
  MC5 — LAUFZEIT: >24h als Tage im Dropdown (48→2d, 168→7d, 720→30d),
        24 h bleibt 24 h; der gesendete Wert bleibt die Stundenzahl.
  MC6 — BILDER: Dateiauswahl → Vorschau-Thumbnail; ✕ entfernt es wieder;
        der Submit-Payload trägt image_data (erstes Bild, Base64).
  MC7 — PAYLOAD: voller Submit (Confirm JA) → marketplace_advertise mit
        type, id_item, currency, quality, is_hidden, availability, image_data.

Datenquelle: smoke-stub.js (JS-Mirror der IPC-Commands; die Rust-Seite ist
durch cargo tests in src-tauri/trade.rs + crates/uex-client belegt).
Screenshots nach docs/screens/mkt-listing-fields/.
"""
import asyncio, base64, json, os, pathlib, sys, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens/mkt-listing-fields"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8796"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9230"))
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

SHOP_OFFERS = json.dumps([offer(1, "Gold Ore (Raw)", "Port Tressler", "sell", 4812, 1200)],
                         separators=(",", ":"))

# Item-Pool für den item_suggest-Stub (UEX /items-Schnittmenge).
ITEM_POOL = json.dumps([
    {"id": 2265, "name": "Kastak Arms Custodian SMG", "subtitle": "Weapons · Kastak Arms"},
    {"id": 2266, "name": "CSP-68H Backpack", "subtitle": "Backpacks · RSI"},
    {"id": 2267, "name": "MedPen", "subtitle": "Medical · CureLife"},
], separators=(",", ":"))

STUB_EXTRA = (
    "window.__stubState.uex_api_key_set = true;"
    "window.__stubState.uex_secret_key_set = true;"
    "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
    "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
    "window.__stubState.marketplace_confirm = 'every_action';"
    "window.__itemPool = " + ITEM_POOL + ";"
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
        "--window-size=1920,1600", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir=/tmp/cdp-mkt-mc",
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
        await call("Emulation.setDeviceMetricsOverride",
                   {"width": 1920, "height": 1700, "deviceScaleFactor": 2, "mobile": False})

        async def navigate():
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB_EXTRA})
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
            await js("(function(){var i=document.querySelector('input.q'); if(i){i.focus();}})()")
            await key("F2")
            await asyncio.sleep(1.0)
            await click_chip("Mein Shop")
            await asyncio.sleep(1.5)

        async def open_form():
            await click_chip("Anlegen")
            await asyncio.sleep(0.8)

        def invoke_watch():
            return js("""(function(){
                var orig = window.__TAURI_INTERNALS__.invoke;
                window.__calls = window.__calls || [];
                window.__TAURI_INTERNALS__.invoke = function(cmd,args){
                    window.__calls.push(cmd + ':' + JSON.stringify(args || {}));
                    return orig(cmd,args);
                };
            })()""")

        FILL_DECL = (
            "function setVal(sel,v){var i=typeof sel==='string'?document.querySelector(sel):sel;"
            "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
            "set.call(i,v);i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:v}));}"
        )
        def fill_expr(js_expr):
            return "(function(){" + FILL_DECL + js_expr + "return true;})()"

        # ---------------------------------------------------------------
        # MC1 — Formular zeigt ALLE UEX-Felder
        # ---------------------------------------------------------------
        await navigate()
        await enter_shop()
        await open_form()
        labels = await js("[...document.querySelectorAll('span.m-lbl')].map(function(x){return (x.textContent||'').trim();})")
        for need in ["Listing Type", "Währung", "Ort", "Item Source", "Quality", "Availability", "Hide Listing"]:
            check(f"mc1-field-{need}", isinstance(labels, list) and need in labels, "labels=" + str(labels)[:200])
        check("mc1-field-Bilder", isinstance(labels, list) and any(l.startswith("Bilder") for l in labels),
              "labels=" + str(labels)[:200])
        listing_sel = await js("document.querySelector('select[aria-label=\"Listing Type\"]') !== null")
        check("mc1-listing-type-select", listing_sel is True)
        currency_vals = await js("[...document.querySelectorAll('select[aria-label=Currency] option')].map(function(o){return o.value;})")
        check("mc1-currency-uec-default", currency_vals == ["UEC", "WIF", "MGS"], str(currency_vals))
        # Item-Feld nur bei Type=Item (Default) sichtbar.
        item_ta = await js("document.querySelector('input[placeholder*=\"Item suchen\"]') !== null")
        check("mc1-item-typeahead-visible", item_ta is True)
        # Defaults: Type=Item, Currency=UEC, Hide=No.
        form_type = await js("document.querySelector('select[aria-label=\"Listing Type\"]').value")
        check("mc1-default-listing-type-item", form_type == "item", "type=" + str(form_type))
        hide_label = await js("(function(){var b=[...document.querySelectorAll('button.chip')].find(function(x){return (x.textContent||'').trim()==='No'&&x.title&&x.title.includes('Verborgen');});return b?b.textContent.trim():null;})()")
        check("mc1-hide-default-no", hide_label == "No", "hide=" + str(hide_label))
        await shot("mc1-form-all-fields.png")

        # ---------------------------------------------------------------
        # MC2 — Ort-Autocomplete (nur Locations, keine Shops)
        # ---------------------------------------------------------------
        await js(fill_expr("setVal('input[placeholder*=\"z. B. Port Tressler\"]','Check');"))
        await asyncio.sleep(0.6)
        dd = await js("document.querySelectorAll('.rta-dd .rta-item').length")
        loc_items = await js("[...document.querySelectorAll('.rta-dd .rta-item')].map(function(x){return (x.textContent||'').trim();})")
        check("mc2-loc-dropdown-opens", int(dd or 0) >= 1, "items=" + str(loc_items))
        only_locs = all(("Shop" not in i and "Admin" not in i and "Guns" not in i) for i in loc_items)
        check("mc2-loc-only-locations", only_locs is True, str(loc_items)[:160])
        # Enter übernimmt die erste Vorschlags-Option.
        await js("(function(){var inp=document.querySelector('input[placeholder*=\"z. B. Port Tressler\"]');"
                 "inp.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}));"
                 "inp.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));})()")
        await asyncio.sleep(0.4)
        loc_val = await js("document.querySelector('input[placeholder*=\"z. B. Port Tressler\"]').value")
        check("mc2-loc-picked", str(loc_val).strip() != "Check", "value=" + str(loc_val))
        await shot("mc2-location-autocomplete.png")

        # ---------------------------------------------------------------
        # MC3 — Item-Typeahead + Auswahl setzt id_item
        # ---------------------------------------------------------------
        await js(fill_expr("setVal('input[placeholder*=\"Item suchen\"]','Custodian');"))
        await asyncio.sleep(0.6)
        item_items = await js("[...document.querySelectorAll('.rta-dd .rta-item')].map(function(x){return (x.textContent||'').trim();})")
        check("mc3-item-suggestions", any("Custodian" in i for i in item_items), str(item_items)[:160])
        await js("(function(){var inp=document.querySelector('input[placeholder*=\"Item suchen\"]');"
                 "inp.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}));"
                 "inp.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));})()")
        await asyncio.sleep(0.4)
        item_label = await js("document.querySelector('input[placeholder*=\"Item suchen\"]').value")
        check("mc3-item-picked-label", "Custodian" in str(item_label), "label=" + str(item_label))
        await shot("mc3-item-autocomplete.png")

        # ---------------------------------------------------------------
        # MC4 — PFLICHT-VALIDIERUNG: leeres Formular → deutsche Meldungen
        # ---------------------------------------------------------------
        await navigate()
        await enter_shop()
        await open_form()
        await invoke_watch()
        # Menge explizit leeren — Default ist '1' (vorbelegt), der
        # Mengen-Pflichtfehler muss mit leerem Wert nachweisbar sein.
        await js(fill_expr(
            "var m=document.querySelectorAll('input.qnum')[1];"
            "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
            "set.call(m,'');m.dispatchEvent(new Event('input',{bubbles:true}));"
        ))
        await click_btn("Anlegen (2-Schritt)")
        await asyncio.sleep(0.6)
        no_confirm = await js("!document.body.innerText.includes('Listing ANLEGEN?')")
        check("mc4-no-confirm-on-invalid", no_confirm is True)
        warn_texts = await js("[...document.querySelectorAll('.chip.warn')].map(function(x){return (x.textContent||'').trim();})")
        for need in ["Titel ist Pflicht.", "Bitte eine Kategorie wählen", "Bitte ein Item wählen", "Ort ist Pflicht.", "Preis muss eine positive Ganzzahl sein.", "Menge muss eine positive Ganzzahl sein."]:
            check(f"mc4-msg-{need[:20]}", any(need in t for t in warn_texts), "warns=" + str(warn_texts)[:200])
        calls4 = json.loads(str(await js("JSON.stringify(window.__calls||[])")) or "[]")
        adv4 = [c for c in calls4 if "marketplace_advertise" in c]
        check("mc4-no-write-on-invalid", len(adv4) == 0, "kein Write")
        await shot("mc4-validation-errors.png")

        # ---------------------------------------------------------------
        # MC5 — Laufzeit-Anzeige: >24h als Tage; Wert bleibt h im Payload
        # ---------------------------------------------------------------
        await navigate()
        await enter_shop()
        await open_form()
        await invoke_watch()
        opts = await js("[...document.querySelectorAll('select[aria-label=Laufzeit] option')].map(function(x){return x.textContent.trim()+'='+x.value;})")
        want = {"1 h=1", "12 h=12", "24 h=24", "2d=48", "7d=168", "30d=720"}
        check("mc5-hour-labels", all(any(o.split('=')[0] == w.split('=')[0] and o.split('=')[1] == w.split('=')[1] for o in opts) for w in want),
              "opts=" + str(opts)[:200])
        # 168 auswählen → der Payload trägt 168 (nicht 7).
        await js("(function(){var s=document.querySelector('select[aria-label=Laufzeit]');"
                 "s.value='168';s.dispatchEvent(new Event('change',{bubbles:true}));})()")
        await js(fill_expr(
            "setVal('input[placeholder*=Artikelname]','MC Laufzeit Test');"
            "setVal('input[placeholder*=Freitext]','Laufzeit-Wert bleibt Stunden.');"
            "setVal(document.querySelectorAll('input.qnum')[0],'150000');"
            "setVal('input[placeholder*=\"z. B. Port Tressler\"]','Port Tressler');"
            "var cat=document.querySelector('select[aria-label=Kategorie]');"
            "if(cat){cat.value=cat.options[1].value;cat.dispatchEvent(new Event('change',{bubbles:true}));}"
            "var item=document.querySelector('input[placeholder*=\"Item suchen\"]');"
            "if(item){item.value='Kastak Arms Custodian SMG';item.dispatchEvent(new Event('input',{bubbles:true}));}"
        ))
        await asyncio.sleep(0.4)
        # Item-Pick über den Typeahead (Enter) — Pflicht bei type=item.
        await js("(function(){var inp=document.querySelector('input[placeholder*=\"Item suchen\"]');"
                 "if(inp&&inp.value){inp.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}));"
                 "inp.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));}})()")
        await asyncio.sleep(0.4)
        await click_btn("Anlegen (2-Schritt)")
        await asyncio.sleep(0.5)
        await click_btn("JA, ANLEGEN")
        await asyncio.sleep(1.2)
        calls5 = json.loads(str(await js("JSON.stringify(window.__calls||[])")) or "[]")
        hours_sent = None
        for c in calls5:
            if "marketplace_advertise" in c:
                try:
                    args = json.loads(c.split(":", 1)[1])
                    hours_sent = args.get("payload", {}).get("hours_expiration")
                except Exception:
                    pass
        check("mc5-hours-sent-as-hours", hours_sent == 168, "hours_expiration=" + str(hours_sent))
        await shot("mc5-duration-label.png")

        # ---------------------------------------------------------------
        # MC6 — BILDER: Dateiauswahl → Vorschau; ✕ entfernt; image_data im Payload
        # ---------------------------------------------------------------
        await navigate()
        await enter_shop()
        await open_form()
        # Ein echtes 1x1-PNG als File in den File-Input injizieren (DataTransfer).
        png = base64.b64decode(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==")
        b64 = base64.b64encode(png).decode()
        added = await js(fill_expr(
            "var dt=new DataTransfer();"
            "var blob=new Blob([Uint8Array.from(atob('" + b64 + "'),function(c){return c.charCodeAt(0);})],{type:'image/png'});"
            "dt.items.add(new File([blob],'thumb.png',{type:'image/png'}));"
            "var inp=document.querySelector('input[type=file]');"
            "inp.files=dt.files;"
            "inp.dispatchEvent(new Event('change',{bubbles:true}));"
        ))
        await asyncio.sleep(0.8)
        thumbs = await js("document.querySelectorAll('img[alt^=Vorschau]').length")
        check("mc6-thumbnail-shown", added is True and int(thumbs or 0) == 1, "thumbs=" + str(thumbs))
        await shot("mc6-image-preview.png")
        # ✕ entfernt das Bild wieder.
        await js("(function(){var b=[...document.querySelectorAll('button.chip.warn')].find(function(x){return (x.textContent||'').trim()==='✕';});if(b){b.click();return true;}return false;})()")
        await asyncio.sleep(0.3)
        thumbs_after = await js("document.querySelectorAll('img[alt^=Vorschau]').length")
        check("mc6-remove-image", int(thumbs_after or 0) == 0, "thumbs=" + str(thumbs_after))

        # ---------------------------------------------------------------
        # MC7 — PAYLOAD: alle neuen Felder gehen doku-konform an advertise
        # ---------------------------------------------------------------
        await js(fill_expr(
            "var dt=new DataTransfer();"
            "var blob=new Blob([Uint8Array.from(atob('" + b64 + "'),function(c){return c.charCodeAt(0);})],{type:'image/png'});"
            "dt.items.add(new File([blob],'thumb.png',{type:'image/png'}));"
            "var inp=document.querySelector('input[type=file]');"
            "inp.files=dt.files;"
            "inp.dispatchEvent(new Event('change',{bubbles:true}));"
        ))
        await asyncio.sleep(0.8)
        await js(fill_expr(
            "setVal('input[placeholder*=Artikelname]','MC Voll Payload');"
            "setVal('input[placeholder*=Freitext]','Alle neuen Felder gesetzt.');"
            "setVal(document.querySelectorAll('input.qnum')[0],'125000');"
            "setVal('input[placeholder*=\"z. B. Port Tressler\"]','Levski');"
            "var cat=document.querySelector('select[aria-label=Kategorie]');"
            "if(cat){cat.value=cat.options[1].value;cat.dispatchEvent(new Event('change',{bubbles:true}));}"
            "var item=document.querySelector('input[placeholder*=\"Item suchen\"]');"
            "if(item){item.value='Kastak Arms Custodian SMG';item.dispatchEvent(new Event('input',{bubbles:true}));}"
            "var typesel=document.querySelector('select[aria-label=\"Listing Type\"]');"
            "if(typesel){typesel.value='item';typesel.dispatchEvent(new Event('change',{bubbles:true}));}"
            "var src=document.querySelector('select[aria-label=\"Item Source\"]');"
            "if(src){src.value='looted';src.dispatchEvent(new Event('change',{bubbles:true}));}"
            "var av=document.querySelector('select[aria-label=Availability]');"
            "if(av){av.value='ready_pickup';av.dispatchEvent(new Event('change',{bubbles:true}));}"
            "var qnum=document.querySelectorAll('input.qnum');"
            "var qnum2=qnum[2];"
            "var set2=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
            "set2.call(qnum2,'85');qnum2.dispatchEvent(new Event('input',{bubbles:true}));"
        ))
        # Hide Listing auf Ja.
        await js("(function(){var b=[...document.querySelectorAll('button.chip')].find(function(x){return x.title&&x.title.includes('Verborgen')&&(x.textContent||'').trim()==='No';});if(b){b.click();return true;}return false;})()")
        # item_id setzen (Typeahead-Pick über onselect) — wir nutzen den
        # Change-JS-Fix: das Item-Typeahead braucht den onselect-Pfad. Wir
        # simulieren den Enter-Pick wie in MC3, nachdem ein Query gesetzt ist.
        await js(fill_expr("setVal('input[placeholder*=\"Item suchen\"]','Custodian');"))
        await asyncio.sleep(0.6)
        await js("(function(){var inp=document.querySelector('input[placeholder*=\"Item suchen\"]');"
                 "inp.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}));"
                 "inp.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));})()")
        await asyncio.sleep(0.4)
        await invoke_watch()
        await click_btn("Anlegen (2-Schritt)")
        await asyncio.sleep(0.5)
        await click_btn("JA, ANLEGEN")
        await asyncio.sleep(1.2)
        adv_calls = [c for c in json.loads(str(await js("JSON.stringify(window.__calls||[])")) or "[]")
                     if "marketplace_advertise" in c]
        check("mc7-advertise-fired", len(adv_calls) >= 1, str(adv_calls[:1]))
        sent = None
        for c in adv_calls:
            try:
                args = json.loads(c.split(":", 1)[1])
                sent = args.get("payload")
            except Exception:
                pass
        check("mc7-type-item", isinstance(sent, dict) and sent.get("type") == "item", str(sent and sent.get("type")))
        check("mc7-id-item-numeric", isinstance(sent, dict) and isinstance(sent.get("id_item"), int) and sent["id_item"] > 0, "id_item=" + str(sent and sent.get("id_item")))
        check("mc7-currency-uec", isinstance(sent, dict) and sent.get("currency") == "UEC", str(sent and sent.get("currency")))
        check("mc7-source-looted", isinstance(sent, dict) and sent.get("source") == "looted", str(sent and sent.get("source")))
        check("mc7-availability", isinstance(sent, dict) and sent.get("availability") == "ready_pickup", str(sent and sent.get("availability")))
        check("mc7-quality-85", isinstance(sent, dict) and sent.get("quality") == 85, "quality=" + str(sent and sent.get("quality")))
        check("mc7-hidden-1", isinstance(sent, dict) and sent.get("is_hidden") == 1, "is_hidden=" + str(sent and sent.get("is_hidden")))
        img = sent and sent.get("image_data")
        check("mc7-image-data-base64", isinstance(img, str) and img.startswith("data:image/png;base64,"), "img-start=" + str((img or "")[:32]))
        await shot("mc7-full-payload-submit.png")

    proc.terminate()
    await proc.wait()
    server.terminate()
    await server.wait()

    print("\n===== MC Listing-Formular Probe =====")
    print(f"PASS {len(PASS)} / FAIL {len(FAIL)}")
    for f in FAIL:
        print("  FAILED:", f)
    sys.exit(1 if FAIL else 0)

if __name__ == "__main__":
    asyncio.run(main())
