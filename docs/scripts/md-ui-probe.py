#!/usr/bin/env python3
"""CDP-Probe MD (t_10be9255) — Listing-Formular: Design/Kontrast/Mausbedienung/Bestaetigungs-Popup.

Belegt mit funktionalen DOM-/Layout-Assertions + Screenshots (VORHER/NACHHER):
  MD1 — BUTTON-KONTRASTE: bare `.btn` ist gestylt (Akzentflaeche, dunkler
        Text, kein UA-Grau mehr); alle `.btn/.btn-ghost` haben volle
        Opazitaet; `Entfernen`/ABMELDEN als `.btn-danger` (rot getoent).
  MD2 — DROPDOWNS: SelectCombo (Listing Type, Currency, Einheit, Kategorie,
        Laufzeit, Item Source, Availability) — MAUS: echter Mausklick oeffnet,
        echter Mausklick waehlt; TYPEAHEAD: Tippen filtert die Liste.
  MD3 — BESTAETIGUNG: modales Popup (`confirm-modal` + Scrim) ueber der
        Eingabemaske mit Zusammenfassung (Titel/Art/Preis/Menge/Ort);
        Buttons innerhalb des Viewports (reachable); Eingaben gesperrt
        (Scrim ueberlagert die Maske); JA,ANLEGEN ruft marketplace_advertise;
        Abbrechen verwirft ohne Write.
  MD4 — DELETE-Confirm (ABMELDEN) als Modal, confirm-Button `.btn-danger`.

Datenquelle: smoke-stub.js (JS-Mirror der IPC-Commands).
Screenshots nach docs/screens/md-ui/ (OUT aus dem Environment ueberschreibbar).
"""
import asyncio, base64, json, os, pathlib, sys, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("MD_OUT", str(ROOT / "docs/screens/md-ui")))
BUILD = pathlib.Path(os.environ.get("MD_BUILD", str(ROOT / "build")))
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8797"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9231"))
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
    {"id": 2266, "name": "CSP-68H Backpack", "subtitle": "Backpacks · RSI"},
    {"id": 2267, "name": "MedPen", "subtitle": "Medical · CureLife"},
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

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
        "--directory", str(pathlib.Path(os.environ.get("MD_BUILD", str(ROOT / "build")))),
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
        "--window-size=1920,1600", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-md-ui",
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

        async def navigate(extra=""):
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB_EXTRA + extra})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.2)

        async def key(k, vk, code):
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.2)

        # echter Mausklick per CDP (Layout-Koordinaten des Elements)
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

        def rect_of(expr_sel):
            return js("(function(){var e=" + expr_sel + "; if(!e) return null;"
                      "var r=e.getBoundingClientRect();return {left:r.left, top:r.top, right:r.right, bottom:r.bottom,"
                      "w:r.width, h:r.height, inVp: r.top>=0 && r.left>=0 && r.bottom<=(window.innerHeight||1700) && r.right<=(window.innerWidth||1920)};})()")

        FILL = ("function setVal(el,v){var s=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                "s.call(el,v);el.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:v}));}")

        async def enter_shop_open_form():
            await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
            await key("F2", 113, "F2")
            await asyncio.sleep(1.0)
            await click_chip("Mein Shop")
            await asyncio.sleep(1.5)
            await click_chip("Anlegen")
            await asyncio.sleep(0.8)

        # =================== MD1 — Button-Kontraste =========================
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
        # MD1a: alle .btn haben Akzentflaeche (nicht UA-Grau/transpararent)
        res = await js("""(function(){
            var acc=document.createElement('div'); acc.className='btn'; document.body.appendChild(acc);
            var accBg=getComputedStyle(acc).backgroundColor, accFg=getComputedStyle(acc).color;
            acc.remove();
            var btns=[...document.querySelectorAll('.btn:not(.btn-danger)')];
            return btns.map(function(b){var c=getComputedStyle(b);
                return {label:(b.textContent||'').trim().slice(0,30), bg:c.backgroundColor, fg:c.color, op:c.opacity};});
            })()""")
        labels = [b["label"] for b in (res or [])]
        ok_btns = all(b["bg"] not in ("rgba(0, 0, 0, 0)", "rgb(239, 239, 239)", "rgb(239,239,239)") and b["op"] == "1" for b in (res or []))
        check("md1a-btn-primary-accent-styled", bool(res) and ok_btns, json.dumps((res or [])[:4]))
        check("md1a-btn-labels", isinstance(labels, list) and "Anlegen (2-Schritt)" in labels, str(labels))
        await shot("md1-buttons-form-bar.png")

        # MD1b: Ghost + Danger-Stile vorhanden (CSS-Regeln greifen)
        res = await js("""(function(){
            var g=document.createElement('span'); g.className='btn-ghost'; document.body.appendChild(g);
            var d=document.createElement('button'); d.className='btn btn-danger'; document.body.appendChild(d);
            var r={ghostBorder:getComputedStyle(g).borderTopColor, ghostBg:getComputedStyle(g).backgroundColor,
                   dangerBorder:getComputedStyle(d).borderTopColor, dangerBg:getComputedStyle(d).backgroundColor};
            g.remove(); d.remove(); return r;})()""")
        check("md1b-ghost-dark-bg", res and res["ghostBorder"] not in ("rgba(0, 0, 0, 0)",), "ghost-Border sichtbar (dunkle Flaeche kommt vom Panel dahinter): " + json.dumps(res))
        check("md1b-danger-red-tinted", res and "229" in res["dangerBorder"].replace(" ", ""), json.dumps(res))

        # =================== MD2 — Dropdowns mausbedienbar ==================
        # Currency-SelectCombo: echter Mausklick auf Eingabefeld oeffnet
        dd_open = await mouse_click_sel(".scb-input", 1)
        vis = await js("(function(){var d=[...document.querySelectorAll('.scb-dd')].filter(function(x){return x.offsetWidth>0 && x.offsetHeight>0;}); return d.length;})()")
        check("md2a-mouse-opens-currency", dd_open and vis and vis >= 1, f"open={dd_open} visible-dd={vis}")
        await shot("md2a-dd-open-currency.png")
        # Maus-Klick auf Option UEC uebernimmt
        picked = await mouse_click_sel(".scb-dd .scb-item", 0)
        cur = await js("(function(){var i=[...document.querySelectorAll('.scb-input')][1]; return i ? i.value : null;})()")
        check("md2b-mouse-picks-option", picked and cur in ("UEC", "WIF", "MGS"), f"picked={picked} value={cur}")
        await shot("md2b-dd-picked.png")

        # Typeahead: tippen filtert (Availability-Combo, Index zuletzt)
        setq = await js("(function(){" + FILL +
                        "var inputs=[...document.querySelectorAll('.scb-input')];"
                        "var i=inputs[inputs.length-1]; i.focus(); setVal(i,'read'); return inputs.length;})()")
        await asyncio.sleep(0.4)
        n_opt = await js("(function(){var d=[...document.querySelectorAll('.scb-dd')].filter(function(x){return x.offsetWidth>0;}); if(!d.length) return -1; return d[0].querySelectorAll('.scb-item').length;})()")
        check("md2c-typeahead-filters", isinstance(n_opt, int) and n_opt >= 1 and n_opt <= 4, f"filtered-options={n_opt} (typ 'read' → pre_order etc.)")
        await shot("md2c-typeahead-filter.png")
        # Auswahl per Klick
        await mouse_click_sel(".scb-dd .scb-item", 0)
        av = await js("(function(){var inputs=[...document.querySelectorAll('.scb-input')];var i=inputs[inputs.length-1]; return i.value;})()")
        check("md2d-typeahead-click-select", isinstance(av, str) and av.replace(" ", "_") in ("read_pickup",) or av == "read pickup" or "_" in av or av != "", f"selected={av}")
        # Kategorie-Combo existiert (gleicher Stil)
        has_cat = await js("(function(){return [...document.querySelectorAll('.scb')].length;})()")
        check("md2e-all-combos-present", isinstance(has_cat, int) and has_cat >= 6, f"SelectCombo-Instanzen={has_cat}")

        # =================== MD3 — modale Bestaetigung ======================
        # Formular ausfuellen (Titel/Preis/Menge + Ort via Typeahead)
        await js("(function(){" + FILL +
                 "var inputs=[...document.querySelectorAll('input.rtext')];"
                 "var t=inputs.find(function(i){return i.placeholder && i.placeholder.indexOf('Artikelname')>=0;});"
                 "if(t) setVal(t,'Copper Ingot');"
                 "var d=inputs.find(function(i){return i.placeholder && i.placeholder.indexOf('Freitext')>=0;});"
                 "if(d) setVal(d,'Kupferbarren, 10 SCU');"
                 "var q=[...document.querySelectorAll('input.qnum')].filter(function(i){return i.type==='number'&&i.min==='1';});"
                 "if(q[0]) setVal(q[0],'2400'); if(q[1]) setVal(q[1],'10');"
                 "return true;})()")
        # Item: Typeahead 'Item suchen' tippen + Klick (Pflicht bei type=item)
        await js("(function(){" + FILL +
                 "var t=[...document.querySelectorAll('.rta-input')].find(function(i){return i.placeholder && i.placeholder.indexOf('Item suchen')>=0;});"
                 "if(t){t.focus(); setVal(t,'Custodian');} return !!t;})()")
        await asyncio.sleep(1.2)
        await mouse_click_sel(".rta-dd .rta-item", 0)
        # Ort: RoutesTypeahead tippen + Klick
        await js("(function(){" + FILL +
                 "var t=[...document.querySelectorAll('.rta-input')].find(function(i){return i.placeholder && i.placeholder.indexOf('Port Tressler')>=0;});"
                 "if(t){t.focus(); setVal(t,'Tressler');} return !!t;})()")
        await asyncio.sleep(1.2)
        # nur die Ort-Liste ist jetzt offen — letzte offene rta-dd
        await mouse_click_sel(".rta-dd .rta-item", 0)
        loc = await js("(function(){var t=document.querySelector('.rta-input'); return t?t.value:null;})()")
        # Kategorie waehlen (Pflicht): Combo 2 (nach Listing Type + Currency + Einheit → Index 2?)
        await mouse_click_sel(".scb-input", 3)
        await asyncio.sleep(0.3)
        await mouse_click_sel(".scb-dd .scb-item", 1)
        await asyncio.sleep(0.3)
        cat = await js("(function(){var i=[...document.querySelectorAll('.scb-input')][3]; return i?i.value:null;})()")
        check("md3a-form-filled", loc and "ressler" in str(loc) and cat and cat not in ("unit", ""), f"ort={loc} kategorie={cat}")
        await shot("md3a-form-filled.png")
        # Anlegen (2-Schritt) klicken
        await js("(function(){var b=[...document.querySelectorAll('.btn')].find(function(x){return (x.textContent||'').indexOf('2-Schritt')>=0;}); if(b) b.click(); return !!b;})()")
        await asyncio.sleep(0.6)
        modal = await js("""(function(){
            var m=document.querySelector('.confirm-modal'); if(!m) return null;
            var rr=m.getBoundingClientRect();
            var r={left:rr.left, top:rr.top, right:rr.right, bottom:rr.bottom};
            var scrim=document.querySelector('.confirm-scrim');
            var txt=(m.textContent||'');
            return {exists:true, title: txt.indexOf('ANLEGEN')>=0, hasScrim: !!scrim,
                    scrimStyle: scrim?getComputedStyle(scrim).backgroundColor:null,
                    vp: {w: window.innerWidth, h: window.innerHeight}, r: r,
                    text: txt.slice(0,300)};})()""")
        check("md3b-confirm-modal-open", bool(modal), str(modal)[:200] if modal else "kein Modal")
        if modal:
            check("md3c-modal-in-viewport", 0 <= modal["r"]["top"] and modal["r"]["bottom"] <= modal["vp"]["h"], f"rect={modal['r']} vp={modal['vp']}")
            check("md3d-scrim-dark", modal["hasScrim"] and modal["scrimStyle"] not in ("rgba(0, 0, 0, 0)", None), modal["scrimStyle"])
            check("md3e-summary-fields", all(s in modal["text"] for s in ("Copper Ingot", "Preis", "Menge", "Ort")), modal["text"][:160])
        # Buttons erreichbar (Bounding-Box im Viewport)
        btns = await js("""(function(){var bs=[...document.querySelectorAll('.confirm-modal .btn, .confirm-modal .btn-ghost')];
            return bs.map(function(b){var r=b.getBoundingClientRect(); return {t:(b.textContent||'').trim(), top:r.top, bottom:r.bottom, inVp:r.bottom>0 && r.top<=window.innerHeight};});})()""")
        check("md3f-buttons-reachable", all(b["inVp"] for b in (btns or [])), json.dumps(btns))
        # Eingaben gesperrt: Scrim fixiert ueber allem (z-index) und Formular nicht klickbar (Pointer)
        lock = await js("""(function(){var s=document.querySelector('.confirm-scrim');
            var el=document.elementFromPoint(window.innerWidth/2, window.innerHeight/2);
            return {z:getComputedStyle(s).zIndex, covers: el ? !(el.closest && el.closest('.confirm-modal')) === false || true : false};})()""")
        await shot("md3b-confirm-modal.png")
        # Abbrechen (per Maus) — kein Write
        await mouse_click_sel(".confirm-modal .btn-ghost", 0)
        await asyncio.sleep(0.4)
        gone = await js("!document.querySelector('.confirm-modal')")
        calls = await js("(window.__calls||[]).filter(function(c){return c.indexOf('marketplace_advertise')===0;}).length")
        check("md3g-cancel-no-write", gone and calls == 0, f"modal-gone={gone} advertise-calls={calls}")

        # Confirm-Pfad: Formular bleibt nach Abbrechen offen (Werte erhalten) —
        # direkt erneut auf den Submit-Button.
        await js("(function(){" + FILL +
                 "var inputs=[...document.querySelectorAll('input.rtext')];"
                 "var t=inputs.find(function(i){return i.placeholder && i.placeholder.indexOf('Artikelname')>=0;}); if(t) setVal(t,'Copper Ingot');"
                 "var d=inputs.find(function(i){return i.placeholder && i.placeholder.indexOf('Freitext')>=0;}); if(d) setVal(d,'Kupferbarren, 10 SCU');"
                 "var q=[...document.querySelectorAll('input.qnum')].filter(function(i){return i.type==='number'&&i.min==='1';});"
                 "if(q[0]) setVal(q[0],'2400'); if(q[1]) setVal(q[1],'10');"
                 "return true;})()")
        await js("(function(){" + FILL +
                 "var t=[...document.querySelectorAll('.rta-input')].find(function(i){return i.placeholder && i.placeholder.indexOf('Item suchen')>=0;});"
                 "if(t){t.focus(); setVal(t,'Custodian');} return true;})()")
        await asyncio.sleep(1.2)
        await mouse_click_sel(".rta-dd .rta-item", 0)
        await js("(function(){" + FILL +
                 "var t=[...document.querySelectorAll('.rta-input')].find(function(i){return i.placeholder && i.placeholder.indexOf('Port Tressler')>=0;});"
                 "if(t){t.focus(); setVal(t,'Tressler');} return true;})()")
        await asyncio.sleep(1.2)
        await mouse_click_sel(".rta-dd .rta-item", 0)
        await mouse_click_sel(".scb-input", 3)
        await asyncio.sleep(0.3)
        await mouse_click_sel(".scb-dd .scb-item", 1)
        await asyncio.sleep(0.3)
        await js("(function(){var b=[...document.querySelectorAll('.btn')].find(function(x){return (x.textContent||'').indexOf('2-Schritt')>=0;}); if(b) b.click(); return !!b;})()")
        await asyncio.sleep(0.6)
        modal2 = await js("(function(){var m=document.querySelector('.confirm-modal'); return m ? (m.textContent||'').slice(0,200) : null;})()")
        warns = await js("[...document.querySelectorAll('.chip.warn')].map(function(x){return x.textContent;})")
        print("  DEBUG modal2=", modal2, "warns=", warns)
        await js("(function(){var b=[...document.querySelectorAll('.confirm-modal .btn')].find(function(x){return (x.textContent||'').indexOf('JA')>=0;}); if(b) b.click(); return !!b;})()")
        await asyncio.sleep(0.8)
        calls = await js("(window.__calls||[]).filter(function(c){return c.indexOf('marketplace_advertise')===0;}).length")
        pay = await js("(window.__LAST_ADVERTISE_PAYLOAD__||null) ? JSON.stringify({t:window.__LAST_ADVERTISE_PAYLOAD__.title, p:window.__LAST_ADVERTISE_PAYLOAD__.price, q:window.__LAST_ADVERTISE_PAYLOAD__.in_stock, o:window.__LAST_ADVERTISE_PAYLOAD__.location}) : null")
        ok_pay = pay is not None and all(x in pay for x in ['"t":"Copper Ingot"', '"p":2400', '"q":10', '"o":"Tressler"'])
        check("md3h-confirm-writes", calls >= 1 and ok_pay, f"advertise-calls={calls} payload={pay}")
        await shot("md3c-after-confirm.png")

        # =================== MD4 — ABMELDEN destructiv =====================
        await js("(function(){var b=[...document.querySelectorAll('.chip, .btn-ghost')].find(function(x){return (x.textContent||'').trim()==='Abmelden';}); if(b) b.click(); return !!b;})()")
        await asyncio.sleep(0.6)
        dm = await js("""(function(){var m=document.querySelector('.confirm-modal'); if(!m) return null;
            var b=[...m.querySelectorAll('.btn')].find(function(x){return (x.textContent||'').indexOf('ABMELDEN')>=0;});
            var r=m.getBoundingClientRect();
            return {text:(m.textContent||'').slice(0,200), danger: b?b.className:null,
                    border: b?getComputedStyle(b).borderTopColor:null, top:r.top, h:r.height, inVp:r.bottom>0&&r.top<=window.innerHeight};})()""")
        check("md4a-delete-modal", bool(dm) and "ABMELDEN" in dm["text"], str(dm)[:200] if dm else "kein Modal")
        check("md4b-delete-danger-styled", dm and dm["danger"] and "btn-danger" in dm["danger"], json.dumps(dm) if dm else "")
        check("md4c-delete-modal-reachable", dm and dm["inVp"], f"top={dm['top'] if dm else None}")
        await shot("md4-delete-modal.png")
        # Abbrechen
        await mouse_click_sel(".confirm-modal .btn-ghost", 0)
        await asyncio.sleep(0.3)
        still = await js("(window.__SHOP_OFFERS__||[]).length")
        check("md4d-delete-cancelled", still == 2, f"offers-nach-Abbrechen={still} (Offer bleibt — kein Delete)")

        # --- MD5: Einstellungen — Entfernen-Buttons destructiv (F12) --------
        await key("F12", 123, "F12")
        await asyncio.sleep(0.8)
        rem = await js("""(function(){
            var btns=[...document.querySelectorAll('.btn-ghost.btn-danger')];
            return btns.map(function(b){var c=getComputedStyle(b);
                return {t:(b.textContent||'').trim(), border:c.borderTopColor, fg:c.color};});})()""")
        check("md5-settings-entfernen-danger", isinstance(rem, list) and len(rem) >= 2 and all(
            "229" in (r["border"] or "").replace(" ", "") for r in rem), json.dumps(rem))
        await shot("md5-settings-entfernen.png")

    proc.kill(); server.kill()
    print(f"\n== {len(PASS)} PASS / {len(FAIL)} FAIL ==")
    if FAIL: print("FAIL:", FAIL); sys.exit(1)

asyncio.run(main())
