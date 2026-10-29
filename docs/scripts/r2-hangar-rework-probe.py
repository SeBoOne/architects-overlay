#!/usr/bin/env python3
"""CDP-Probe für t_2158474c (R2: Hangar-Rework — Popup, Typeahead-Einheit,
SelectCombo, Module in Ship-Details, dd.mm.jjjj).

Datenquelle: synthetischer Hangar-Stand über window.__HANGAR_DATA__ (Stub) —
2 Schiffe, 1 Fahrzeug, 1 verlinktes Modul (base_ship=„600i Explorer“), 1
unverbundenes Modul (Legacy-Bestand, base_ship=leer). Nur die IPC-Grenze ist
gestubbt (smoke-stub.js), die echte Svelte-UI läuft unverändert.

Checks:
  1. F11 → Hangar-List (6 Rows).
  2. + Anlegen → modales Popup (.hform-modal im .advert-scrim), vollständig
     innerhalb der Overlay-Grenzen (Buttons sichtbar), KEIN natives <select>
     mehr im Formular (Typ/Herkunft/Versicherung = SelectCombo).
  3. Name-Typeahead: 'cut' → rta-dd Vorschläge, ArrowDown+Enter übernimmt
     'Cutlass Black' (Hersteller automatisch Drake).
  4. Versicherung-Select: Optionen Ohne/3/6/12/24/120 Monate/LTI.
  5. Datum dd.mm.jjjj: Eingabe 31.01.2025 → Bestätigungs-Popup zeigt
     '31.01.2025', JA → Stub-Zeile speichert ISO '2025-01-31'.
  6. Detail: KEINE Ship-Code/Pledge/Pledge-ID/Warbond/CCUD-Zeilen; Datum als
     dd.mm.jjjj.
  7. Module in Ship-Details: 600i Explorer zeigt Modul (1) mit Entfernen-✕;
     Hinzufügen per Autocomplete → Modul (2) + Stub-Zeile entity_type=module
     mit base_ship; Entfernen → Modul (1).
  8. Module-Filter: zeigt die Schiffe MIT Modulen (600i Explorer), nicht die
     Modul-Zeilen selbst.
  9. Unverbundenes Modul: Detail zeigt 'Modul (unverbunden)' + Zuordnen
     (Basisschiff-Typeahead) → hangar_update setzt base_ship.
 10. Bearbeiten öffnet dasselbe Popup mit dd.mm.jjjj-Datum.

Screenshots: docs/screenshots/hangar-rework/01…10-*.png
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/hangar-rework")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8796/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + str(detail)) if detail and not ok else ""))

def mk(id, name, origin, insurance, entity_type, base_ship="", manufacturer="",
       price=None, date="", custom_name=""):
    return {"id": id, "name": name, "origin": origin, "insurance": insurance,
            "custom_name": custom_name, "entity_type": entity_type,
            "base_ship": base_ship, "ship_code": "", "manufacturer": manufacturer,
            "purchase_price": price, "purchase_date": date, "pledge_id": "",
            "pledge_name": "", "warbond": False, "ccud": False,
            "created_at": None, "updated_at": None}

ITEMS = [
    mk(1, "Cutlass Black", "pledge", "LTI", "ship", manufacturer="Drake Interplanetary", price=180.0, date="2024-07-12"),
    mk(2, "Aurora Mk I SE", "pledge", "3 Monate", "ship", manufacturer="RSI", price=75.0, date="2026-03-24"),
    mk(3, "600i Explorer", "pledge", "LTI", "ship", manufacturer="Origin Jumpworks", price=475.0, date="2024-07-12"),
    mk(4, "Mule", "ingame", None, "vehicle", manufacturer="Drake"),
    mk(5, "600i Exploration Module", "pledge", "LTI", "module", base_ship="600i Explorer", price=180.0, date="2024-07-12"),
    mk(6, "Golem OX Modul", "pledge", None, "module"),
]
N_ROWS = len(ITEMS)

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
        "--remote-debugging-port=9233", "--user-data-dir=/tmp/cdp-hangar-r2",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9233/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        server.terminate(); proc.terminate()
        raise RuntimeError("CDP did not come up")

    try:
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
            await call("Page.addScriptToEvaluateOnNewDocument",
                       {"source": STUB + "\nwindow.__HANGAR_DATA__ = " + json.dumps(ITEMS) + ";"})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + (r["exceptionDetails"].get("text") or "")
                return r.get("result", {}).get("value")

            VK = {"F11": 122, "Escape": 27, "Enter": 13, "ArrowDown": 40}
            async def key(k):
                vk = VK[k]
                await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                      "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
                await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                      "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
                await asyncio.sleep(0.3)

            async def shot(name):
                r = ok(await call("Page.captureScreenshot", {"format": "png"}))
                (OUT / name).write_bytes(base64.b64decode(r["data"]))
                print("  shot:", OUT / name)

            async def setfield(sel, val):
                return await js(f"(() => {{ const e = document.querySelector('{sel}'); if (!e) return 'NOSLCT';"
                                f"e.value = {json.dumps(val)}; e.dispatchEvent(new Event('input', {{bubbles: true}}));"
                                f"return e.value; }})()")

            async def rows():
                return await js("document.querySelectorAll('.hangar-list .hrow').length")

            def H(expr):
                return js("(() => { const m = document.querySelector('.hform-modal'); if (!m) return 'NOFORM'; " + expr + " })()")

            # ---- 1: F11 → Hangar-List ----
            await key("F11")
            await asyncio.sleep(1.0)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'") or "").strip()
            check("f11-enters-hangar", smode == "HANGAR", smode)
            check(f"list-{N_ROWS}-rows", (await rows()) == N_ROWS, "rows=" + str(await rows()))
            await shot("01-hangar-list.png")

            # ---- 2: Popup ----
            await js("[...document.querySelectorAll('.hangar-actions .hbtn')].find(b=>b.innerText.includes('Anlegen')).click()")
            await asyncio.sleep(0.5)
            scrim = await js("document.querySelector('.advert-scrim .hform-modal') ? 'yes' : 'no'")
            check("popup-modal-open", scrim == "yes", scrim)
            geo = await js("""
                const m = document.querySelector('.hform-modal');
                const r = m.getBoundingClientRect();
                JSON.stringify({top: r.top, bottom: r.bottom, left: r.left, right: r.right,
                                vh: window.innerHeight, vw: window.innerWidth,
                                btn: !!m.querySelector('.hform-foot button[type=submit]')})""")
            g = json.loads(geo)
            check("popup-inside-overlay", g["top"] >= 0 and g["left"] >= 0 and g["bottom"] <= g["vh"] and g["right"] <= g["vw"], geo)
            check("popup-save-button-visible", g["btn"] and g["bottom"] <= g["vh"], geo)
            native = await H("return m.querySelectorAll('select').length")
            check("popup-no-native-select", native == 0, native)
            await shot("02-popup-open.png")
            # Beleg „alles sichtbar": Body an das Ende scrollen — Datum/
            # Custom-Name/Hersteller + Buttons innerhalb des Viewports.
            await js("(() => { const b = document.querySelector('.hform-body'); b.scrollTop = b.scrollHeight; return 'OK'; })()")
            await asyncio.sleep(0.3)
            lowvis = await js("(() => { const m = document.querySelector('.hform-modal'); const f = [...m.querySelectorAll('.trk-field')].find(x => x.textContent.indexOf('Hersteller') >= 0); const b = m.querySelector('.hform-foot'); if (!f || !b) return 'NOF'; const fr = f.getBoundingClientRect(), br = b.getBoundingClientRect(); return JSON.stringify({fBottom: Math.round(fr.bottom), bBottom: Math.round(br.bottom), vh: window.innerHeight}); })()")
            lv = json.loads(lowvis) if lowvis and str(lowvis).startswith("{") else {}
            check("popup-all-fields-visible-on-scroll", bool(lv) and lv["fBottom"] <= lv["vh"] and lv["bBottom"] <= lv["vh"], lowvis)
            await shot("02b-popup-scroll-bottom.png")

            # ---- 3: Name-Typeahead (Maus + Tastatur) ----
            inp = "await0"
            r = await js("(() => { const i = document.querySelector('.hform-modal .rta-input'); if (!i) return 'NOINPUT';"
                         "i.focus(); i.value='cut'; i.dispatchEvent(new Event('input',{bubbles:true})); return 'OK'; })()")
            await asyncio.sleep(0.4)
            items = await js("[...document.querySelectorAll('.hform-modal .rta-dd .rta-item')].map(b=>b.innerText.trim())")
            check("name-typeahead-suggestions", bool(items) and any("Cutlass" in s for s in (items or [])), str(items))
            # Tastatur: ArrowDown + Enter übernimmt den Vorschlag
            await key("ArrowDown")
            await key("Enter")
            await asyncio.sleep(0.3)
            nameval = await js("(() => { const i=document.querySelector('.hform-modal .rta-input'); return i ? i.value : 'NOINPUT'; })()")
            man = await H("const i=[...m.querySelectorAll('.trk-field')].find(f=>f.textContent.includes('Hersteller')); return i ? i.querySelector('input').value : 'NOSLCT'")
            # Pick trifft Option 2 (Cutlass Blue, Katalog — Hersteller-Kurzname 'Drake').
            check("name-typeahead-keyboard-pick-manufacturer", (man or "") == "Drake", f"man={man!r} name={nameval!r}")
            await shot("03-name-typeahead.png")

            # ---- 4: Versicherung-Select ----
            await js("(() => { const f=[...document.querySelectorAll('.hform-modal .trk-field')].find(x=>x.textContent.includes('Versicherung')); f.querySelector('.scb-input').click(); return 'OK'; })()")
            await asyncio.sleep(0.3)
            ins = await js("[...document.querySelectorAll('.hform-modal .scb-dd .scb-item')].map(b=>b.innerText.trim())")
            check("insurance-options", ins == ["Ohne", "3 Monate", "6 Monate", "12 Monate", "24 Monate", "120 Monate", "LTI"], str(ins))
            await shot("04-insurance-select.png")
            await js("(() => { const f=[...document.querySelectorAll('.hform-modal .trk-field')].find(x=>x.textContent.includes('Versicherung')); const i=f.querySelector('.scb-input'); i.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})); return 'OK'; })()")
            await asyncio.sleep(0.2)

            # ---- 5: Datum dd.mm.jjjj → ISO ----
            await js("(() => { const f=[...document.querySelectorAll('.hform-modal .trk-field')].find(x=>x.textContent.includes('Datum'));"
                     "const i=f.querySelector('input'); i.focus(); i.value='31.01.2025';"
                     "i.dispatchEvent(new Event('input',{bubbles:true})); return 'OK'; })()")
            await asyncio.sleep(0.2)
            await js("document.querySelector('.hform-modal button[type=submit]').click()")
            await asyncio.sleep(0.5)
            conf = await js("(() => { const c=document.querySelector('.advert-scrim .confirm-modal'); return c ? c.innerText.slice(0, 200) : 'NOCONF'; })()")
            check("confirm-modal-open", "EINTRAG ANLEGEN" in str(conf), conf)
            check("confirm-shows-de-date", "31.01.2025" in str(conf), conf)
            await shot("05-confirm-popup.png")
            await js("[...document.querySelectorAll('.confirm-modal button')].find(b=>b.innerText.includes('JA, SPEICHERN')).click()")
            await asyncio.sleep(0.8)
            saved = await js("(() => { const it = window.__HANGAR_DATA__.find(i=>i.purchase_date==='2025-01-31'); return it ? it.name + '|ISO-OK' : 'MISSING'; })()")
            check("date-saved-as-iso", "ISO-OK" in str(saved), saved)
            check("popup-closed-after-save", (await js("document.querySelector('.hform-modal') ? 'open' : 'closed'")) == "closed")

            # ---- 6: Detail ohne Import-Artefakte, Datum dd.mm.jjjj ----
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('600i Explorer')).click()")
            await asyncio.sleep(0.5)
            dl = await js("(() => { const d=document.querySelector('.hangar-detail'); return d ? d.innerText : 'NODETAIL'; })()")
            dl = str(dl)
            check("detail-no-shipcode", "Ship-Code" not in dl)
            check("detail-no-pledge", "Pledge-ID" not in dl and "Pledge" not in dl, dl[:120])
            check("detail-no-warbond-ccud", "Warbond" not in dl and "CCUD" not in dl)
            check("detail-date-de-format", "12.07.2024" in dl, dl[:300])
            await shot("06-detail-clean.png")

            # ---- 7: Module in Ship-Details (add/remove) ----
            mods0 = await js("(() => { const s=document.querySelector('.hd-mods .h'); return s ? s.innerText : 'NOMODS'; })()")
            check("ship-detail-module-section", "Module (1)" in str(mods0), mods0)
            await js("(() => { const f=document.querySelector('.hd-mods .hmod-add .rta-input'); if(!f) return 'NOINPUT';"
                     "f.focus(); f.value='Exploration'; f.dispatchEvent(new Event('input',{bubbles:true})); return 'OK'; })()")
            await asyncio.sleep(0.4)
            moditems = await js("[...document.querySelectorAll('.hd-mods .hmod-add .rta-dd .rta-item')].map(b=>b.innerText.trim())")
            check("module-autocomplete-from-registered", bool(moditems) and any("Exploration Module" in s for s in (moditems or [])), str(moditems))
            if isinstance(moditems, list) and moditems:
                await js("[...document.querySelectorAll('.hd-mods .hmod-add .rta-dd .rta-item')][0].click()")
                await asyncio.sleep(0.3)
            await js("[...document.querySelectorAll('.hd-mods .hmod-add .hbtn')].find(b=>b.innerText.includes('Hinzufügen')).click()")
            await asyncio.sleep(0.8)
            mods1 = await js("(() => { const s=document.querySelector('.hd-mods .h'); return s ? s.innerText : 'NOMODS'; })()")
            check("module-added", "Module (2)" in str(mods1), mods1)
            newmod = await js("(() => { const it = window.__HANGAR_DATA__.find(i=>i.entity_type==='module' && i.base_ship==='600i Explorer' && i.id>6); return it ? it.name : 'MISSING'; })()")
            check("module-row-created-with-base-ship", newmod == "600i Exploration Module", newmod)
            await shot("07-module-added.png")
            await js("[...document.querySelectorAll('.hd-mods .hmod-x')][1].click()")
            await asyncio.sleep(0.8)
            mods2 = await js("(() => { const s=document.querySelector('.hd-mods .h'); return s ? s.innerText : 'NOMODS'; })()")
            check("module-removed", "Module (1)" in str(mods2), mods2)
            await key("Escape")  # zurück zur Liste
            await asyncio.sleep(0.3)

            # ---- 8: Module-Filter zeigt Schiffe MIT Modulen ----
            await js("[...document.querySelectorAll('.hangar-filters .hchip')].find(b=>b.innerText.includes('MODULE')).click()")
            await asyncio.sleep(0.4)
            frows = await js("[...document.querySelectorAll('.hangar-list .hrow .hname')].map(b=>b.innerText.trim())")
            check("module-filter-shows-ships-with-modules", frows == ["600i Explorer"], str(frows))
            await shot("08-module-filter.png")
            await js("[...document.querySelectorAll('.hangar-filters .hchip')].find(b=>b.innerText.includes('ALLE')).click()")
            await asyncio.sleep(0.3)

            # ---- 9: Unverbundenes Modul → Zuordnen ----
            unbound = await js("(() => { const r=[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('unverbunden')); return r ? r.innerText.trim() : 'MISSING'; })()")
            check("unbound-module-labeled", "unverbunden" in str(unbound), unbound)
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('unverbunden')).click()")
            await asyncio.sleep(0.5)
            assign = await js("(() => { const d=document.querySelector('.hangar-detail'); return d ? d.innerText : 'NODETAIL'; })()")
            check("unbound-detail-shows-assign", "Modul (unverbunden)" in str(assign) and "Zuordnen" in str(assign), str(assign)[:200])
            await js("(() => { const f=document.querySelector('.hd-mods .hmod-add .rta-input'); f.focus(); f.value='Cutlass'; f.dispatchEvent(new Event('input',{bubbles:true})); return 'OK'; })()")
            await asyncio.sleep(0.4)
            await js("[...document.querySelectorAll('.hd-mods .hmod-add .rta-dd .rta-item')][0].click()")
            await asyncio.sleep(0.3)
            await js("[...document.querySelectorAll('.hd-mods .hmod-add .hbtn')].find(b=>b.innerText.includes('Zuordnen')).click()")
            await asyncio.sleep(0.8)
            assigned = await js("(() => { const it = window.__HANGAR_DATA__.find(i=>i.id===6); return it ? (it.base_ship || 'EMPTY') : 'MISSING'; })()")
            check("unbound-module-assigned", assigned == "Cutlass Black", assigned)
            await shot("09-module-assigned.png")

            # ---- 10: Bearbeiten öffnet dasselbe Popup mit dd.mm.jjjj ----
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('Cutlass Black')).click()")
            await asyncio.sleep(0.4)
            await js("[...document.querySelectorAll('.hangar-detail .hbtn')].find(b=>b.innerText.includes('Bearbeiten')).click()")
            await asyncio.sleep(0.5)
            etitle = await js("(() => { const l=document.querySelector('.hform-modal .m-lbl'); return l ? l.innerText : 'NOFORM'; })()")
            check("edit-opens-same-popup", etitle == "EINTRAG BEARBEITEN", etitle)
            edate = await js("(() => { const f=[...document.querySelectorAll('.hform-modal .trk-field')].find(x=>x.textContent.includes('Datum')); return f ? f.querySelector('input').value : 'NOINPUT'; })()")
            check("edit-date-de-format", edate == "12.07.2024", edate)
            await shot("10-edit-popup-de-date.png")
            # Abbrechen (Delete-Confirm-Pfad wird über denselben Mechanismus bedient)
            await js("[...document.querySelectorAll('.hform-modal .hform-foot .hbtn')].find(b=>b.innerText.includes('Abbrechen')).click()")
            await asyncio.sleep(0.3)
    finally:
        for p_ in (server, proc):
            try:
                p_.terminate()
            except ProcessLookupError:
                pass

    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit("FAILED: " + ", ".join(FAIL))

asyncio.run(main())
