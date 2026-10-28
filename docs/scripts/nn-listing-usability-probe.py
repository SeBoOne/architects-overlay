#!/usr/bin/env python3
"""CDP-Probe NN (t_060d134e) — Listing-Formular: Bedienbarkeit/Platzhalter.

Belegt mit funktionalen DOM-/Layout-Assertions + Screenshots:
  NN1 — DROPDOWNS LOESCHBAR: geleert man ein SelectCombo (Währung) und
        verlaesst das Feld, bleibt es LEER (kein Auto-Reinsert); bei Fokus
        zeigt es die KOMPLETTE Optionsliste; erst eine Auswahl setzt wieder
        einen Wert. (Typeahead bleibt: Tippen filtert.)
  NN2 — TITEL & PREIS BREIT: Titel-Input >= 400px (kein Abschneiden),
        Preis-Input >= 150px; Eingabe '1500000000' wird als
        '1.500.000.000' angezeigt und als 1500000000 gesendet.
  NN3 — QUALITAET 0-1000: Input max=1000, placeholder '0–1000'; 970 ist
        valide, 1001 liefert den Validierungs-Hinweis (Payload enthaelt 970).
  NN4 — NUR 1 BILD: File-Input ohne `multiple`; zwei gewaehlte Bilder
        ersetzen einander (images bleibt length 1); Vorschau + Entfernen
        vorhanden; Payload image_data = das Bild.
  NN5 — ROUTEN-PLACEHOLDER: Schiff-Typeahead in Handelsrouten zeigt
        'z. B. Ironclad' (statt 'Type-ahead Schiff').

Datenquelle: smoke-stub.js (JS-Mirror der IPC-Commands).
Screenshots nach docs/screens/nn-listing-usability/ (OUT ueberschreibbar).
"""
import asyncio, base64, json, os, pathlib, sys, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("NN_OUT", str(ROOT / "docs/screens/nn-listing-usability")))
BUILD = pathlib.Path(os.environ.get("NN_BUILD", str(ROOT / "build")))
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8797"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9235"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

NOW = int(time.time())
def offer(id_, title, location, op, price, qty):
    return {
        "id": id_, "title": title, "description": "",
        "item_id": id_, "category_id": 1, "operation": op,
        "listing_type": "item", "price": price, "currency": "UEC", "unit": "scu",
        "quantity": qty, "location": location, "region": "Stanton",
        "seller": "sebo_one", "quality": 850, "updated": NOW - 3600,
        "expires": None, "slug": f"sebo-{id_}"
    }

SHOP_OFFERS = json.dumps([offer(1, "Gold Ore (Raw)", "Port Tressler", "sell", 4812, 1200)], separators=(",", ":"))
ITEM_POOL = json.dumps([
    {"id": 2265, "name": "Kastak Arms Custodian SMG", "subtitle": "Weapons · Kastak Arms"},
], separators=(",", ":"))

STUB_EXTRA = (
    "window.__stubState.uex_api_key_set = true;"
    "window.__stubState.uex_secret_key_set = true;"
    "window.__stubState.uex_username = 'sebo_one';"
    "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
    "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"
    "window.__itemPool = " + ITEM_POOL + ";"
    "window.__stubState.marketplace_confirm = 'every_action';"
)

# 1x1-PNG (transparenz) als Test-Bild
PNG1PX = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
        "--directory", str(BUILD),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    import urllib.request
    for _ in range(60):
        try:
            with urllib.request.urlopen(URL, timeout=1) as r:
                if r.status == 200: break
        except Exception: pass
        await asyncio.sleep(0.25)
    else:
        raise RuntimeError("static server did not come up")
    print("static server up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1700", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-nn-listing",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{CDP_PORT}/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception: pass
        await asyncio.sleep(0.25)
    if ws is None:
        raise RuntimeError("chromium CDP not up")

    async with websockets.connect(ws, max_size=None) as cdp:
        msg_id = 0; pending = {}
        async def call(method, params=None):
            nonlocal msg_id
            msg_id += 1; mid = msg_id
            fut = asyncio.get_event_loop().create_future(); pending[mid] = fut
            await cdp.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
            return await fut
        async def recv_loop():
            async for raw in cdp:
                m = json.loads(raw)
                if "id" in m and m["id"] in pending: pending[m["id"]].set_result(m)
        asyncio.create_task(recv_loop())
        def ok(r):
            if "error" in r: raise RuntimeError("CDP error: %s" % r["error"])
            return r.get("result", {})

        await call("Page.enable"); await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride",
                   {"width": 1920, "height": 1700, "deviceScaleFactor": 2, "mobile": False})

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + str(r["exceptionDetails"].get("text"))
            return r.get("result", {}).get("value")

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", name)

        async def key(k, vk, code):
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.2)

        async def mouse_click_sel(sel, idx=0):
            rect = await js(
                "(function(){var e=document.querySelectorAll(" + json.dumps(sel) + ")[" + str(idx) + "];"
                "if(!e) return null; var r=e.getBoundingClientRect();"
                "return {x:(r.left+r.right)/2, y:(r.top+r.bottom)/2};})()")
            if not rect: return False
            await call("Input.dispatchMouseEvent", {"type": "mousePressed", "x": rect["x"], "y": rect["y"], "button": "left", "clickCount": 1})
            await call("Input.dispatchMouseEvent", {"type": "mouseReleased", "x": rect["x"], "y": rect["y"], "button": "left", "clickCount": 1})
            await asyncio.sleep(0.4)
            return True

        async def click_chip(label):
            return await js("(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return (x.textContent||'').trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()")

        FILL = ("function setVal(el,v){var s=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                "s.call(el,v);el.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:v}));}")

        async def navigate(extra=""):
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB_EXTRA + extra})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.2)

        async def enter_shop_open_form():
            await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
            await key("F2", 113, "F2")
            await asyncio.sleep(1.0)
            await click_chip("Mein Shop")
            await asyncio.sleep(1.5)
            await click_chip("Anlegen")
            await asyncio.sleep(0.8)

        async def fill_form(price="1500000000", quality="970"):
            await js("(function(){" + FILL +
                     "var inputs=[...document.querySelectorAll('input.rtext')];"
                     "var t=inputs.find(function(i){return i.placeholder && i.placeholder.indexOf('Artikelname')>=0;});"
                     "if(t) setVal(t,'Atzkav Sniper Rifle 970 Q Crafted ! \\\\ Most Powerful in game Sniper Rifle');"
                     "var d=inputs.find(function(i){return i.placeholder && i.placeholder.indexOf('Freitext')>=0;});"
                     "if(d) setVal(d,'Sniper, crafted, 970Q');"
                     "return true;})()")
            # Preis: Text-Input mit Tausender-Formatierung (placeholder 'z. B. 1.500.000.000')
            await js("(function(){" + FILL +
                     "var p=[...document.querySelectorAll('input.qnum')].find(function(i){return i.type==='text'&&i.inputMode==='numeric';});"
                     "if(p){p.focus(); setVal(p,'" + price + "');} return !!p;})()")
            # Quality: Number-Input mit placeholder '0–1000'
            await js("(function(){" + FILL +
                     "var q=[...document.querySelectorAll('input.qnum')].find(function(i){return i.type==='number'&&i.placeholder.indexOf('1000')>=0;});"
                     "if(q){q.focus(); setVal(q,'" + quality + "');} return !!q;})()")
            await js("(function(){" + FILL +
                     "var q=[...document.querySelectorAll('input.qnum')].find(function(i){return i.type==='number'&&i.min==='1';});"
                     "if(q) setVal(q,'10'); return true;})()")
            # Item + Ort
            await js("(function(){" + FILL +
                     "var t=[...document.querySelectorAll('.rta-input')].find(function(i){return i.placeholder && i.placeholder.indexOf('Item suchen')>=0;});"
                     "if(t){t.focus(); setVal(t,'Custodian');} return !!t;})()")
            await asyncio.sleep(1.2)
            await mouse_click_sel(".rta-dd .rta-item", 0)
            await js("(function(){" + FILL +
                     "var t=[...document.querySelectorAll('.rta-input')].find(function(i){return i.placeholder && i.placeholder.indexOf('Port Tressler')>=0;});"
                     "if(t){t.focus(); setVal(t,'Tressler');} return true;})()")
            await asyncio.sleep(1.2)
            await mouse_click_sel(".rta-dd .rta-item", 0)
            # Kategorie (Combo-Index 3)
            await mouse_click_sel(".scb-input", 3)
            await asyncio.sleep(0.3)
            await mouse_click_sel(".scb-dd .scb-item", 1)
            await asyncio.sleep(0.3)

        # =================== NN1 — Dropdowns löschbar =======================
        await navigate()
        # IPC-Watch: marketplace_advertise-Aufrufe zaehlen
        await js("""(function(){
            var orig = window.__TAURI_INTERNALS__.invoke;
            window.__calls = window.__calls || [];
            window.__TAURI_INTERNALS__.invoke = function(cmd,args){
                window.__calls.push(cmd + ':' + JSON.stringify(args || {}));
                return orig(cmd,args);
            };
        })()""")
        await enter_shop_open_form()
        # Currency-Combo per aria-label identifizieren (kein Index-Globus —
        # andere Combos können offen sein und .scb-dd-Duplikate liefern).
        CURFIND = "var i=[...document.querySelectorAll('.scb-input')].find(function(x){return x.getAttribute('aria-label')==='Currency';});"
        async def cur_val():
            return await js("(function(){" + CURFIND + "return i?i.value:null;})()")
        cur0 = await cur_val()
        check("nn1a-default-value-set", cur0 == "UEC", f"value={cur0!r}")
        cleared = await js("(function(){" + FILL + CURFIND +
                           "i.focus(); setVal(i,''); return i.value;})()")
        # Blur (echtes blur) → KEIN Auto-Reinsert
        await js("(function(){" + CURFIND + "if(i) i.blur();})()")
        await asyncio.sleep(0.5)
        cur1 = await cur_val()
        check("nn1b-empty-stays-empty", cleared == "" and cur1 == "", f"nach Leer+Blur: {cur1!r} (kein Reinsert)")
        # Fokus → komplette Optionsliste sichtbar (alle 3 Währungen)
        await js("(function(){" + CURFIND + "if(i) i.focus();})()")
        await asyncio.sleep(0.4)
        opts = await js("(function(){var d=[...document.querySelectorAll('.scb-dd')].filter(function(x){return x.offsetWidth>0;});"
                        "if(!d.length) return null; return [...d[0].querySelectorAll('.scb-item')].map(function(b){return (b.textContent||'').trim();});})()")
        check("nn1c-empty-shows-all-options", isinstance(opts, list) and opts == ["UEC", "WIF", "MGS"], str(opts))
        await shot("nn1-empty-dropdown-all-options.png")
        # Auswahl setzt wieder einen Wert: sichtbaren Currency-dd-Eintrag 'WIF' klicken
        picked = await js("(function(){var d=[...document.querySelectorAll('.scb-dd')].filter(function(x){return x.offsetWidth>0;})[0];"
                          "if(!d) return false; var b=[...d.querySelectorAll('.scb-item')].find(function(x){return (x.textContent||'').trim()==='WIF';});"
                          "if(b){b.click(); return true;} return false;})()")
        await asyncio.sleep(0.3)
        cur2 = await cur_val()
        check("nn1d-select-sets-value", picked and cur2 == "WIF", f"picked={picked} value={cur2!r}")
        # Typeahead bleibt: tippen filtert
        await js("(function(){" + FILL + CURFIND + " i.focus(); setVal(i,'UE');})()")
        await asyncio.sleep(0.3)
        n_opt = await js("(function(){var d=[...document.querySelectorAll('.scb-dd')].filter(function(x){return x.offsetWidth>0;});"
                         "if(!d.length) return null; return [...d[0].querySelectorAll('.scb-item')].map(function(b){return (b.textContent||'').trim();});})()")
        # 'UE' filtert auf UEC; der aktuell gewählte Wert (WIF aus nn1d)
        # bleibt gemäß Filter-Regel zusätzlich sichtbar → 2 Einträge.
        check("nn1e-typeahead-still-filters", isinstance(n_opt, list) and set(n_opt) == {"UEC", "WIF"}, str(n_opt))
        await js("(function(){var d=[...document.querySelectorAll('.scb-dd')].filter(function(x){return x.offsetWidth>0;})[0];"
                 "if(!d) return; var b=[...d.querySelectorAll('.scb-item')].find(function(x){return (x.textContent||'').trim()==='UEC';}); if(b) b.click();})()")

        # =================== NN2 — Titel & Preis breit ======================
        dims = await js("""(function(){
            var t=[...document.querySelectorAll('input.rtext')].find(function(i){return i.placeholder && i.placeholder.indexOf('Artikelname')>=0;});
            var p=[...document.querySelectorAll('input.qnum')].find(function(i){return i.type==='text'&&i.inputMode==='numeric';});
            if(!t||!p) return null;
            return {tw:t.getBoundingClientRect().width, pw:p.getBoundingClientRect().width};})()""")
        check("nn2a-title-wide", dims and dims["tw"] >= 400, f"Titel-Breite={dims['tw'] if dims else None}px")
        check("nn2b-price-wide", dims and dims["pw"] >= 150, f"Preis-Breite={dims['pw'] if dims else None}px")
        # Tausender-Formatierung: '1500000000' → '1.500.000.000'
        fmt = await js("(function(){" + FILL +
                       "var p=[...document.querySelectorAll('input.qnum')].find(function(i){return i.type==='text'&&i.inputMode==='numeric';});"
                       "if(!p) return null; p.focus(); setVal(p,'1500000000'); return p.value;})()")
        check("nn2c-price-thousands-format", fmt == "1.500.000.000", f"Anzeige={fmt!r}")
        await shot("nn2-wide-fields.png")
        # Beleg: breite Felder MIT Beispieltext (langer Atzkav-Titel + 1,5 Mrd)
        # — VOR jeder Validierung (die erst beim Submit Warn-Chips setzt).
        await js("(function(){" + FILL +
                 "var inputs=[...document.querySelectorAll('input.rtext')];"
                 "var t=inputs.find(function(i){return i.placeholder && i.placeholder.indexOf('Artikelname')>=0;});"
                 "if(t){t.focus(); setVal(t,'Atzkav Sniper Rifle 970 Q Crafted ! \\\\ Most Powerful in game Sniper Rifle');} return !!t;})()")
        await js("(function(){" + FILL +
                 "var q=[...document.querySelectorAll('input.qnum')].find(function(i){return i.type==='number'&&i.placeholder.indexOf('1000')>=0;});"
                 "if(q){q.focus(); setVal(q,'970');} return !!q;})()")
        clip = await js("""(function(){
            var t=[...document.querySelectorAll('input.rtext')].find(function(i){return i.placeholder && i.placeholder.indexOf('Artikelname')>=0;});
            if(!t) return null; return {tw:t.getBoundingClientRect().width, sw:t.scrollWidth, cw:t.clientWidth,
                val:t.value, clipped:t.scrollWidth>t.clientWidth+1};})()""")
        check("nn2d-title-not-clipped", clip and clip["clipped"] is False and len(str(clip["val"])) > 60,
              f"Breite={clip['tw'] if clip else None}px scrollW={clip['sw'] if clip else None} val_len={len(str(clip['val'])) if clip else 0}")
        warns2 = await js("[...document.querySelectorAll('.chip.warn')].map(function(x){return (x.textContent||'').trim();}).join('|')")
        print("  DEBUG nn2 warns:", warns2)
        await shot("nn2b-fields-filled.png")

        # =================== NN3 — Quality 0–1000 ===========================
        qattr = await js("""(function(){
            var q=[...document.querySelectorAll('input.qnum')].find(function(i){return i.type==='number'&&i.placeholder.indexOf('1000')>=0;});
            if(!q) return null; return {max:q.max, min:q.min, placeholder:q.placeholder};})()""")
        check("nn3a-quality-max-1000", qattr and qattr["max"] == "1000" and qattr["placeholder"] == "0–1000", str(qattr))
        # 1001 → Validierungs-Hinweis; 970 → ok (im Submit-Pfad unten geprüft)
        await js("(function(){" + FILL +
                 "var q=[...document.querySelectorAll('input.qnum')].find(function(i){return i.type==='number'&&i.placeholder.indexOf('1000')>=0;});"
                 "if(q){q.focus(); setVal(q,'1001');} return true;})()")
        await js("(function(){var b=[...document.querySelectorAll('.btn')].find(function(x){return (x.textContent||'').indexOf('2-Schritt')>=0;}); if(b) b.click(); return !!b;})()")
        await asyncio.sleep(0.5)
        warn = await js("[...document.querySelectorAll('.chip.warn')].map(function(x){return (x.textContent||'').trim();}).join('|')")
        check("nn3b-quality-1001-rejected", isinstance(warn, str) and "1000" in warn, f"warn={warn!r}")
        await shot("nn3-quality-warn-1001.png")

        # =================== NN4 — nur 1 Bild (Replace) =====================
        fattr = await js("(function(){var f=document.querySelector('input[type=file][accept*=image]');"
                         "return f?{multiple:f.multiple}:null;})()")
        check("nn4a-file-input-single", fattr and fattr["multiple"] is False, str(fattr))
        setfile = ("(function(){var f=document.querySelector('input[type=file][accept*=image]');"
                   "var dt=new DataTransfer();"
                   "dt.items.add(new File([new Uint8Array([1,2,3])],'a.png',{type:'image/png'}));")
        # Bild 1 wählen
        await js(setfile + "f.files=dt.files; f.dispatchEvent(new Event('change',{bubbles:true}));})()")
        await asyncio.sleep(0.8)
        imgs1 = await js("(function(){var pv=[...document.querySelectorAll('img[alt^=\"Vorschau\"]')];"
                         "return {count:pv.length, hasRemove:!!document.querySelector('button[title=\"Bild entfernen\"]')};})()")
        check("nn4b-first-image-preview", imgs1 and imgs1["count"] == 1 and imgs1["hasRemove"], str(imgs1))
        await shot("nn4-one-image.png")
        # Bild 2 wählen → ERSETZT (weiterhin 1)
        await js(setfile + "dt.items.add(new File([new Uint8Array([4,5,6])],'b.png',{type:'image/png'}));"
                 "f.files=dt.files; f.dispatchEvent(new Event('change',{bubbles:true}));})()")
        await asyncio.sleep(0.8)
        imgs2 = await js("(function(){var pv=[...document.querySelectorAll('img[alt^=\"Vorschau\"]')]; return pv.length;})()")
        check("nn4c-second-image-replaces", imgs2 == 1, f"Vorschau-nach-2ter-Wahl={imgs2}")

        # =================== NN2/NN3 Submit → Payload =======================
        # Formular vollständig füllen (Titel/Preis/Menge/Item/Ort/Kategorie,
        # Quality 970) und Submit → Confirm → Payload prüfen.
        await fill_form(price="1500000000", quality="970")
        # Beleg-Screen: breite Felder MIT Beispieltext (langer Titel,
        # 1.500.000.000, Quality 970, 1 Bild-Vorschau).
        warns2 = await js("[...document.querySelectorAll('.chip.warn')].map(function(x){return (x.textContent||'').trim();}).join('|')")
        print("  DEBUG nn2 warns:", warns2)
        await shot("nn2b-fields-filled.png")
        await js("(function(){" + FILL +
                 "var q=[...document.querySelectorAll('input.qnum')].find(function(i){return i.type==='number'&&i.placeholder.indexOf('1000')>=0;});"
                 "if(q){q.focus(); setVal(q,'970');} return true;})()")
        await js("(function(){var b=[...document.querySelectorAll('.btn')].find(function(x){return (x.textContent||'').indexOf('2-Schritt')>=0;}); if(b) b.click(); return !!b;})()")
        await asyncio.sleep(0.6)
        modal = await js("(function(){var m=document.querySelector('.confirm-modal'); return m?(m.textContent||''):null;})()")
        warns5 = await js("[...document.querySelectorAll('.chip.warn')].map(function(x){return (x.textContent||'').trim();}).join('|')")
        print("  DEBUG nn5 warns:", warns5)
        check("nn5a-confirm-modal", isinstance(modal, str) and "Atzkav" in modal, ((modal or "")[:120]) + " warns=" + str(warns5))
        await shot("nn5-confirm-modal.png")
        await js("(function(){var b=[...document.querySelectorAll('.confirm-modal .btn')].find(function(x){return (x.textContent||'').indexOf('JA')>=0;}); if(b) b.click(); return !!b;})()")
        await asyncio.sleep(0.8)
        pay = await js("(function(){var p=window.__LAST_ADVERTISE_PAYLOAD__; return p?{"
                       "price:p.price, quality:p.quality, img:(p.image_data||'').slice(0,30), title:p.title}:null;})()")
        ok_pay = (isinstance(pay, dict) and pay["price"] == 1500000000 and pay["quality"] == 970
                  and pay["img"].startswith("data:") and "Atzkav" in pay["title"])
        check("nn5b-payload-price-quality-image", ok_pay, str(pay)[:200])
        calls = await js("(function(){return (window.__calls||[]).filter(function(c){return c.indexOf('marketplace_advertise')===0;}).length;})()")
        check("nn5c-advertise-once", calls == 1, f"advertise-calls={calls}")

        # =================== NN6 — Routen-Placeholder =======================
        await key("F6", 117, "F6")
        await asyncio.sleep(1.2)
        ph = await js("(function(){var i=[...document.querySelectorAll('.rta-input')].find("
                      "function(x){return x.getAttribute('aria-label')&&x.getAttribute('aria-label').indexOf('Schiff')>=0;});"
                      "return i?i.placeholder:null;})()")
        check("nn6-routes-ship-placeholder", ph == "z. B. Ironclad", f"placeholder={ph!r}")
        await shot("nn6-routes-placeholder.png")

    proc.kill(); server.kill()
    print(f"\n== {len(PASS)} PASS / {len(FAIL)} FAIL ==")
    if FAIL: print("FAIL:", FAIL); sys.exit(1)

asyncio.run(main())
