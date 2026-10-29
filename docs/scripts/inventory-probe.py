#!/usr/bin/env python3
"""CDP-Probe für t_1bab1fea (Inventar-Modus: lokale Bestände, Qualitäten, Standorte).

Belegt auf der echten Svelte-Oberfläche (echte Logik, Tauri-IPC gestubbt):
  1. F4 öffnet den Inventar-Modus (kein Platzhalter mehr), Rail-Aktivzustand.
  2. Gesamtübersicht: nach Standort gruppiert (Heim-Hangar / Meine Carrack /
     Org-Depot), Gold Q900 vs Q364 sichtbar, Fußzeile zählt Positionen.
  3. Haupt-Suchfeld = In-Context-Filter (Standort/Material), Sortier-Chips
     Standort/Menge/Qualität.
  4. Item-Detail: alle Qualitäten/Standorte von Gold + Summen (15 SCU).
  5. CRUD: Erfassen (Entity-Pick via 'search'-Stub + Freitext-Fall),
     Bearbeiten (Enter), Löschen (Entf) — Stub-Datensatz verändert sich.
  6. Esc-Kette: Formular → Übersicht; Item-Detail → Übersicht;
     Übersicht → zurück zur Suche (SMODE SUCHE). F3 bleibt Routen (kein Inventar-Reentry).
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/inventory")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8796/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8796", "--bind", "127.0.0.1",
        "--directory", str(ROOT / "build"),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    for _ in range(60):
        try:
            with urllib.request.urlopen(URL, timeout=1) as r:
                if r.status == 200:
                    break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    else:
        raise RuntimeError("static server did not come up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9230", "--user-data-dir=/tmp/cdp-inventory",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9230/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        server.terminate(); proc.terminate()
        raise RuntimeError("chromium CDP did not come up")

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
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        VK = {"F1": 112, "F2": 113, "F3": 114, "F4": 115, "F12": 123, "Escape": 27, "Enter": 13,
              "ArrowDown": 40, "ArrowUp": 38, "Delete": 46}
        async def key(k):
            vk = VK.get(k, 0)
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.3)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", OUT / name)

        def set_input_expr(sel, text):
            return ("(function(){var i=document.querySelector(" + json.dumps(sel) + ");"
                    "if(!i) return 'NO-ELEMENT';"
                    "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                    "set.call(i,'');"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                    "set.call(i," + json.dumps(text) + ");"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                    "return i.value;})()")

        def smode():
            return js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")

        # ---- 1. F4 → Inventar-Modus (echter View, kein Platzhalter) ----
        # F4 seit Mode-Remap (konfigurierbare Shortcuts): F3 = Routen.
        await js("var i=document.querySelector('input.q'); if(i) i.focus();")
        await key("F4")
        check("f4-opens-inventory", (await smode()).strip() == "INVENTAR", await smode())
        inv = await js("!!document.querySelector('.pane-inventory')")
        check("inventory-view-present", bool(inv))
        active = await js("document.querySelector('.rentry.is-on .rkey') ? document.querySelector('.rentry.is-on .rkey').textContent.trim() : 'NONE'")
        check("rail-f4-active", active == "F4", active)
        ph = await js("!!document.querySelector('.pane-placeholder')")
        check("no-placeholder-anymore", not ph)

        # ---- 2. Gesamtübersicht: Gruppen nach Standort ----
        await asyncio.sleep(0.4)
        groups = json.loads(await js("JSON.stringify([...document.querySelectorAll('.vgroup')].map(function(e){return e.childNodes[0].textContent.trim();}))"))
        check("grouped-by-location", groups == ["Heim-Hangar", "Meine Carrack", "Org-Depot"], str(groups))
        # Zweite Gruppierungsebene: Item-Subgroups unter jedem Standortkopf.
        subs = int(await js("document.querySelectorAll('.inv-list .isub').length"))
        check("overview-item-subgroups", subs >= 3, "subs=" + str(subs))
        gold900 = await js("!![...document.querySelectorAll('.inv-row .q')].some(function(e){return e.innerText.trim()==='Q900';})")
        gold364 = await js("!![...document.querySelectorAll('.inv-row .q')].some(function(e){return e.innerText.trim()==='Q364';})")
        check("gold-q900-and-q364-visible", bool(gold900 and gold364))
        foot = (await js("document.querySelector('.inv-foot') ? document.querySelector('.inv-foot').innerText : 'NONE'")).replace("\n", " · ")
        check("footer-counts-3-positions", "3 Positionen" in foot and "2 Materialien" in foot, foot)
        await shot("01-inventar-uebersicht.png")

        # ---- 3. Filter über das Haupt-Suchfeld ----
        await js(set_input_expr("input.q", "gold"))
        await asyncio.sleep(0.5)
        rows = int(await js("document.querySelectorAll('.inv-row').length"))
        check("filter-gold-hides-titanium", rows == 2, "rows=" + str(rows))
        await js(set_input_expr("input.q", "depot"))
        await asyncio.sleep(0.5)
        rows = int(await js("document.querySelectorAll('.inv-row').length"))
        check("filter-location-matches", rows == 1, "rows=" + str(rows))
        await js(set_input_expr("input.q", ""))
        await asyncio.sleep(0.3)

        # ---- Sortier-Chips: Menge ↓ (Titanium 40 zuerst), Qualität ↓ (900 zuerst) ----
        await js("var c=[...document.querySelectorAll('.inv-bar .chip')].find(function(e){return e.innerText.toLowerCase().indexOf('menge')>=0;}); if(c) c.click();")
        await asyncio.sleep(0.3)
        first = (await js("document.querySelector('.inv-row .loc') ? document.querySelector('.inv-row .loc').innerText : 'NONE'")).split("\n")[0].strip()
        check("sort-qty-titanium-first", first.startswith("Titanium"), first)
        await js("var c=[...document.querySelectorAll('.inv-bar .chip')].find(function(e){return e.innerText.toLowerCase().indexOf('qualität')>=0;}); if(c) c.click();")
        await asyncio.sleep(0.3)
        firstq = (await js("document.querySelector('.inv-row .q') ? document.querySelector('.inv-row .q').innerText : 'NONE'")).strip()
        check("sort-quality-900-first", firstq == "Q900", firstq)
        await js("var c=[...document.querySelectorAll('.inv-bar .chip')].find(function(e){return e.innerText.toLowerCase().indexOf('standort')>=0;}); if(c) c.click();")
        await asyncio.sleep(0.3)

        # ---- 4. Item-Detail (Klick auf den Materialnamen der Gold-Zeile) ----
        await js("var rows=[...document.querySelectorAll('.inv-row')]; var g=rows.find(function(r){return r.innerText.indexOf('Gold')>=0;}); if(g) g.querySelector('.main').click();")
        await asyncio.sleep(0.4)
        det = await js("!!document.querySelector('.inv-detail')")
        check("item-detail-open", bool(det))
        qcounts = json.loads(await js("JSON.stringify([...document.querySelectorAll('.inv-detail .q')].map(function(e){return e.innerText.trim();}))"))
        check("item-detail-shows-900-and-364", "Q900" in qcounts and "Q364" in qcounts, str(qcounts))
        sums = (await js("document.querySelector('.if-sums') ? document.querySelector('.if-sums').innerText : 'NONE'")).replace("\n", " · ")
        check("item-detail-total-15", "15" in sums, sums)
        await shot("02-inventar-item-detail.png")

        # ---- 5. CRUD ----
        # Entf löscht die fokussierte Position (Item-Detail, Gold Q364 fokussiert? sel=0 → Q900)
        await js("window.__lastInv = null; if(document.activeElement && document.activeElement.blur) document.activeElement.blur();")
        await key("Delete")
        await asyncio.sleep(0.4)
        op = await js("globalThis.__lastInv ? globalThis.__lastInv.op : 'none'")
        check("delete-key-removes-row", op == "delete", str(op))
        n = int(await js("(globalThis.__TAURI_INTERNALS__.invoke('inventory_list') instanceof Promise) ? 0 : 0"))  # noop guard
        # Esc: Item-Detail → Übersicht
        await key("Escape")
        await asyncio.sleep(0.3)
        back = await js("!!document.querySelector('.inv-bar') && !document.querySelector('.inv-detail')")
        check("esc-item-detail-back-to-overview", bool(back))
        # ---- 4b. Standort-Detail-View: Gruppenkopf anklicken öffnet die
        # Standort-View (Positionen dort, Item-Subgroups, Esc-Zurück). ----
        await js("var g=document.querySelector('.vgroup'); if(g) g.click();")
        await asyncio.sleep(0.4)
        det_title = await js("document.querySelector('.inv-detail .if-title') ? document.querySelector('.inv-detail .if-title').innerText : 'NONE'")
        check("location-detail-opens", bool(await js("!!document.querySelector('.inv-detail')")) and str(det_title).strip() not in ("", "NONE"), str(det_title))
        det_rows = int(await js("document.querySelectorAll('.inv-detail .inv-row').length"))
        det_subs = int(await js("document.querySelectorAll('.inv-detail .isub').length"))
        check("location-detail-shows-positions", det_rows >= 1, "rows=" + str(det_rows))
        check("location-detail-item-subgroups", det_subs >= 1, "subs=" + str(det_subs))
        await shot("03-inventar-standort-detail.png")
        await key("Escape")
        await asyncio.sleep(0.3)
        locback = await js("!!document.querySelector('.inv-bar') && !document.querySelector('.inv-detail')")
        check("esc-location-detail-back-to-overview", bool(locback))
        # Enter in der Übersicht öffnet das Material-Detail, Enter im Detail
        # öffnet das Bearbeiten-Formular (Gold Q900, sel=0).
        await key("Enter")
        await asyncio.sleep(0.4)
        check("enter-overview-opens-item-detail", bool(await js("!!document.querySelector('.inv-detail')")))
        await key("Enter")
        await asyncio.sleep(0.4)
        form = await js("!!document.querySelector('.inv-form')")
        check("enter-detail-opens-edit-form", bool(form))
        name_val = (await js("document.querySelector('.if-chosen') ? document.querySelector('.if-chosen').innerText : 'NONE'")).replace("\n", " ")
        check("edit-form-prefilled-gold", "Gold" in name_val and "(Freitext)" not in name_val, name_val)
        # Qualität ändern: 900 → 950, speichern
        await js(set_input_expr(".inv-form input[type='number']", "950"))
        await asyncio.sleep(0.2)
        save = await js("var b=[...document.querySelectorAll('.if-actions .btn')].find(function(e){return e.innerText.toLowerCase().indexOf('speichern')>=0;}); if(b) b.click(); true;")
        await asyncio.sleep(0.5)
        op = await js("globalThis.__lastInv ? globalThis.__lastInv.op : 'none'")
        check("edit-saves-update", op == "update", str(op))
        newq = await js("globalThis.__lastInv ? globalThis.__lastInv.row.quality : -1")
        check("edit-quality-950", int(newq or -1) == 950, str(newq))
        # Formular-Esc = Abbrechen
        await key("ArrowDown"); await key("ArrowUp"); await key("Enter")
        await asyncio.sleep(0.3)
        await key("Escape")
        await asyncio.sleep(0.3)
        check("esc-closes-form", not bool(await js("!!document.querySelector('.inv-form')")))
        # Esc nochmal: Item-Detail → Übersicht (die Bearbeitung kam aus dem Detail)
        await key("Escape")
        await asyncio.sleep(0.3)

        # Hinzufügen: Freitext-Position
        await js("var c=document.querySelector('.inv-bar .chip.add'); if(c) c.click();")
        await asyncio.sleep(0.4)
        check("add-form-opens", bool(await js("!!document.querySelector('.inv-form')")))
        # Entity-Pick: "Laranite" suchen → Stub liefert commodity-Treffer
        await js(set_input_expr(".inv-form .if-sect:first-child .if-input", "laranite"))
        await asyncio.sleep(0.8)
        picks = int(await js("document.querySelectorAll('.if-picks .if-pick').length"))
        check("entity-picker-hits", picks >= 1, "picks=" + str(picks))
        await js("var p=document.querySelector('.if-picks .if-pick'); if(p) p.click();")
        await js(set_input_expr(".inv-form input[type='number']", "880"))
        # Zahl-Inputs: Qualität (1.) und Menge (2.) — Qualität ist das erste
        # Keyboard-Pfad: ↓/Enter wählt den Vorschlag, Fokus springt ins
        # Qualität-Feld (keyboard-first Erfassen).
        await js("document.querySelector('.if-picks .if-pick') && document.querySelectorAll('.if-picks .if-pick')[0].focus();")
        await key("ArrowDown")
        await key("Enter")
        kb_focus = await js("document.activeElement && document.activeElement.placeholder || ''")
        check("keyboard-enter-chooses-entity-focuses-quality", "900" in str(kb_focus), str(kb_focus))
        await js(set_input_expr(".inv-form .if-grid .if-sect:first-child .if-input", "880"))
        await js(set_input_expr(".inv-form .if-grid .if-sect:last-child .if-input", "2"))
        await js(set_input_expr(".inv-form input[list='inv-locs']", "Heim-Hangar"))
        await js("var b=[...document.querySelectorAll('.if-actions .btn')].find(function(e){return e.innerText.toLowerCase().indexOf('erfassen')>=0;}); if(b) b.click();")
        await asyncio.sleep(0.5)
        op = await js("globalThis.__lastInv ? globalThis.__lastInv.op : 'none'")
        check("add-saves-position", op == "add", str(op))
        added = json.loads(await js("JSON.stringify(globalThis.__lastInv ? {name:globalThis.__lastInv.row.name, q:globalThis.__lastInv.row.quality, loc:globalThis.__lastInv.row.location_name, qty:globalThis.__lastInv.row.quantity, kind:globalThis.__lastInv.row.entity_kind} : {})"))
        check("add-row-shape", added.get("name") == "Laranite" and added.get("q") == 880 and added.get("loc") == "Heim-Hangar" and added.get("kind") == "commodity", str(added))

        # ---- 5b. Standort-Entity-Pick mit realer Wiki-Id (> 2^53): die
        # String-Id muss UNANGETASTET in den Payload übernommen werden
        # (Number() rundet sie still — t_1bab1fea Blocker 2). ----
        await js("window.__lastInv = null;")
        await js("var c=document.querySelector('.inv-bar .chip.add'); if(c) c.click();")
        await asyncio.sleep(0.4)
        check("add2-form-opens", bool(await js("!!document.querySelector('.inv-form')")))
        await js(set_input_expr(".inv-form .if-sect:first-child .if-input", "gold"))
        await asyncio.sleep(0.8)
        await js("var p=document.querySelectorAll('[aria-label=\"Entity-Treffer\"] .if-pick')[0]; if(p) p.click();")
        await asyncio.sleep(0.2)
        await js(set_input_expr(".inv-form .if-grid .if-sect:first-child .if-input", "500"))
        await js(set_input_expr(".inv-form .if-grid .if-sect:last-child .if-input", "1"))
        await js(set_input_expr(".inv-form input[list='inv-locs']", "mining"))
        await asyncio.sleep(0.9)
        loc_picks = await js("JSON.stringify([...document.querySelectorAll('[aria-label=\"Standort-Entities\"] .if-pick')].map(function(e){return e.innerText.trim();}))")
        check("location-picker-shows-wiki-claim", "Mining Claim" in str(loc_picks), str(loc_picks))
        await js("var b=[...document.querySelectorAll('[aria-label=\"Standort-Entities\"] .if-pick')].find(function(e){return e.innerText.indexOf('Mining Claim')>=0;}); if(b) b.click();")
        await asyncio.sleep(0.2)
        await js("var b=[...document.querySelectorAll('.if-actions .btn')].find(function(e){return e.innerText.toLowerCase().indexOf('erfassen')>=0;}); if(b) b.click();")
        await asyncio.sleep(0.5)
        op2 = await js("globalThis.__lastInv ? globalThis.__lastInv.op : 'none'")
        check("add2-saves-position", op2 == "add", str(op2))
        added2 = json.loads(await js("JSON.stringify(globalThis.__lastInv ? {loc:globalThis.__lastInv.row.location_name, lid:globalThis.__lastInv.row.location_id} : {})"))
        check("location-id-string-passthrough", added2.get("lid") == "9218937313948769813", str(added2))
        await key("Escape")

        # ---- 6. Esc-Kette: Übersicht → Suche; F3 bleibt Routen (kein Inventar-Reentry) ----
        await key("Escape")
        await asyncio.sleep(0.3)
        check("esc-overview-back-to-search", (await smode()).strip() == "SUCHE", await smode())
        focus = await js("document.activeElement === document.querySelector('input.q')")
        check("focus-back-in-search", bool(focus))
        await key("F3")
        check("f3-still-not-inventory", (await smode()).strip() == "HANDELSROUTEN", await smode())
        await key("F1")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS, {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())
