#!/usr/bin/env python3
"""CDP-Probe für t_78a4a5e5 (TB: Formulare leer starten + Hangar: Module nur
in Ship-Details mit Versicherung/Herkunft + Preise raus).

Datenquelle: synthetischer Hangar-Stand über window.__HANGAR_DATA__ (Stub).
Nur die IPC-Grenze ist gestubbt (smoke-stub.js), die echte Svelte-UI läuft
unverändert.

Checks:
  A) Hangar-Liste (Punkt 2/3):
     A1 verlinktes Modul (base_ship gesetzt) NICHT in der Liste,
     A2 unverbundenes Modul bleibt sichtbar („Modul (unverbunden)“),
     A3 KEIN Preis in Listen-Zeilen (kein .hprice, kein '$').
  B) Hangar-Formular leer starten (Punkt 1) + ohne Preis (Punkt 3):
     B1 Typ/Herkunft starten leer mit Platzhalter,
     B2 aufklappen zeigt die KOMPLETTE Optionsliste ohne Tippen,
     B3 Speichern ohne Wahl → Validierungs-Meldung (Typ, dann Herkunft),
     B4 kein Preis-Feld im Formular, keine Preis-Zeile im Confirm,
     B5 nach Wahl+Speichern: Stub-Zeile mit entity_type/origin, price null.
  C) Module in Ship-Details (Punkt 2):
     C1 Modulzeile zeigt Versicherung + Herkunft (Chips),
     C2 Modul-Add ohne Herkunft → Meldung; mit Herkunft/Versicherung →
        Stub-Zeile (module, base_ship, origin, insurance),
     C3 Klick auf Modulname → Edit-Popup: Typ statisch „Modul“, Versicherung
        + Herkunft editierbar und gespeichert.
  D) Module-Filter zeigt Schiffe MIT Modulen.
  E) Listing-Formular leer starten (Punkt 1): Listing Type / Währung /
     Einheit / Laufzeit starten leer mit Platzhalter; Währung/Laufzeit
     zeigen aufklappend die KOMPLETTE Optionsliste.

Screenshots: docs/screenshots/forms-hangar-cleanup/01…07-*.png
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/forms-hangar-cleanup")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail: object = ""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + str(detail)) if detail and not ok else ""))

def mk(id, name, origin, insurance, entity_type, base_ship="", manufacturer="",
       date="", custom_name=""):
    # TB (Punkt 3): purchase_price wird nie mehr geführt — im Seed fest null.
    return {"id": id, "name": name, "origin": origin, "insurance": insurance,
            "custom_name": custom_name, "entity_type": entity_type,
            "base_ship": base_ship, "ship_code": "", "manufacturer": manufacturer,
            "purchase_price": None, "purchase_date": date, "pledge_id": "",
            "pledge_name": "", "warbond": False, "ccud": False,
            "created_at": None, "updated_at": None}

ITEMS = [
    mk(1, "Cutlass Black", "pledge", "LTI", "ship", manufacturer="Drake Interplanetary", date="2024-07-12"),
    mk(2, "Aurora Mk I SE", "pledge", "3 Monate", "ship", manufacturer="RSI", date="2026-03-24"),
    mk(3, "600i Explorer", "pledge", "LTI", "ship", manufacturer="Origin Jumpworks", date="2024-07-12"),
    mk(4, "Mule", "ingame", None, "vehicle", manufacturer="Drake"),
    # Verlinkt → TB-2: KEIN Listen-Eintrag mehr, nur Ship-Details.
    mk(5, "600i Exploration Module", "pledge", "LTI", "module", base_ship="600i Explorer", date="2024-07-12"),
    # Bezugslos → bleibt sichtbar („Modul (unverbunden)“, R2-Vereinbarung).
    mk(6, "Golem OX Modul", "pledge", None, "module"),
]

FOR_SHIP = {
    "600i explorer": {
        "known": True, "has_ports": True,
        "modules": [
            {"name": "600i Exploration Module", "category": "Utility"},
            {"name": "600i Observation Module", "category": "Utility"},
        ],
    }
}
MODUL_STATUS = {
    "version": "module-overrides.json", "synced_at": 1760000000,
    "age_seconds": 5400, "module_count": 2, "ship_count": 1,
}

STUB_EXTRA = "\nwindow.__stubState.uex_secret_key_set = true;"

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8797", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9235", "--user-data-dir=/tmp/cdp-hangar-tb",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9235/json/list", timeout=1) as r:
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
                       {"source": STUB + "\nwindow.__HANGAR_DATA__ = " + json.dumps(ITEMS) + ";" +
                        "\nwindow.__MODUL_FOR_SHIP__ = " + json.dumps(FOR_SHIP) + ";" +
                        "\nwindow.__MODUL_STATUS__ = " + json.dumps(MODUL_STATUS) + ";" +
                        "\nwindow.__stubState.uex_secret_key_set = true;" +
                        "\nwindow.__stubState.uex_api_key_set = true;"})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + (r["exceptionDetails"].get("text") or "")
                return r.get("result", {}).get("value")

            VK = {"F11": 122, "F2": 113, "Escape": 27, "Enter": 13, "ArrowDown": 40}
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

            def H(expr):
                return js("(() => { const m = document.querySelector('.hform-modal'); if (!m) return 'NOFORM'; " + expr + " })()")

            async def toast():
                return await js("(() => { const t=document.querySelector('.hangar-toast .ht-msg'); return t ? t.innerText.trim() : 'NOTOAST'; })()")

            # Feld-Wrapper: .trk-field per Label-Text finden (Hangar-Popup).
            def field_js(label, inner):
                return ("(() => { const f=[...document.querySelectorAll('.hform-modal .trk-field')]"
                        ".find(x=>x.textContent.includes('" + label + "')); if(!f) return 'NOFIELD'; " + inner + " })()")

            # =============================================================
            # A) Hangar-Liste: Module nur in Ship-Details, keine Preise
            # =============================================================
            await key("F11")
            await asyncio.sleep(1.0)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'") or "").strip()
            check("f11-enters-hangar", smode == "HANGAR", smode)
            rowtexts = await js("[...document.querySelectorAll('.hangar-list .hrow')].map(b=>b.innerText)")
            rowtexts = rowtexts or []
            nships = len([i for i in ITEMS if i["entity_type"] != "module"])
            check("tb-list-hides-linked-module", len(rowtexts) == nships + 1 and
                  not any("600i Exploration Module" in t for t in rowtexts),
                  f"rows={len(rowtexts)} expect={nships + 1}")
            check("tb-unbound-still-visible", any("unverbunden" in t for t in rowtexts), rowtexts)
            no_price = await js("(() => ({n: document.querySelectorAll('.hangar-list .hprice').length,"
                                " dollar: [...document.querySelectorAll('.hangar-list .hrow')].some(b=>b.innerText.includes('$'))}))()")
            check("tb-list-no-price", no_price and no_price["n"] == 0 and no_price["dollar"] is False, no_price)
            await shot("01-hangar-list-no-modules-no-price.png")

            # =============================================================
            # B) Hangar-Formular: Felder starten LEER, keine Preis-Felder
            # =============================================================
            await js("[...document.querySelectorAll('.hangar-actions .hbtn')].find(b=>b.innerText.includes('Anlegen')).click()")
            await asyncio.sleep(0.5)
            typ = await js(field_js("Typ", "const i=f.querySelector('.scb-input'); return JSON.stringify({v:i.value, ph:i.placeholder});"))
            check("tb-typ-starts-empty", typ and json.loads(typ)["v"] == "" and "wählen" in json.loads(typ)["ph"], typ)
            herk = await js(field_js("Herkunft", "const i=f.querySelector('.scb-input'); return JSON.stringify({v:i.value, ph:i.placeholder});"))
            check("tb-herkunft-starts-empty", herk and json.loads(herk)["v"] == "" and "wählen" in json.loads(herk)["ph"], herk)
            insv = await js(field_js("Versicherung", "const i=f.querySelector('.scb-input'); return JSON.stringify({v:i.value, ph:i.placeholder});"))
            check("tb-versicherung-starts-empty", insv and json.loads(insv)["v"] == "", insv)
            # B2: aufklappen OHNE Tippen zeigt die KOMPLETTE Liste (Typ).
            await js(field_js("Typ", "f.querySelector('.scb-input').click(); return 'OK';"))
            await asyncio.sleep(0.3)
            typopts = await js("[...document.querySelectorAll('.hform-modal .scb-dd .scb-item')].map(b=>b.innerText.trim())")
            check("tb-typ-full-options-on-open", typopts == ["Schiff", "Fahrzeug"], str(typopts))
            await js(field_js("Typ", "const i=f.querySelector('.scb-input'); i.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})); return 'OK';"))
            await asyncio.sleep(0.2)
            # B4: kein Preis-Feld im Formular.
            pricefield = await H("return [...m.querySelectorAll('.trk-lbl')].some(l=>l.textContent.includes('Preis'));")
            check("tb-form-no-price-field", pricefield is False, pricefield)
            await shot("02-hangar-form-empty-fields.png")
            # B3: Validierung entscheidet — erst Typ, dann Herkunft.
            await js(field_js("Name", "const i=f.querySelector('input'); i.focus(); i.value='Probe Frachter';"
                              "i.dispatchEvent(new Event('input',{bubbles:true})); return 'OK';"))
            await asyncio.sleep(0.2)
            await js("document.querySelector('.hform-modal button[type=submit]').click()")
            await asyncio.sleep(0.4)
            t1 = await toast()
            check("tb-validate-typ-required", "Typ" in str(t1), t1)
            # Typ wählen (Dropdown offen lassen und Schiff picken).
            await js(field_js("Typ", "f.querySelector('.scb-input').click(); return 'OK';"))
            await asyncio.sleep(0.3)
            await js("[...document.querySelectorAll('.hform-modal .scb-dd .scb-item')].find(b=>b.innerText.trim()==='Schiff').click()")
            await asyncio.sleep(0.2)
            await js("document.querySelector('.hform-modal button[type=submit]').click()")
            await asyncio.sleep(0.4)
            t2 = await toast()
            check("tb-validate-herkunft-required", "Herkunft" in str(t2), t2)
            # Herkunft wählen (Task-Nomenklatur: Pledged / In-Game).
            await js(field_js("Herkunft", "f.querySelector('.scb-input').click(); return 'OK';"))
            await asyncio.sleep(0.3)
            herkopts = await js("[...document.querySelectorAll('.hform-modal .scb-dd .scb-item')].map(b=>b.innerText.trim())")
            check("tb-herkunft-options", herkopts == ["In-Game", "Pledged"], str(herkopts))
            await js("[...document.querySelectorAll('.hform-modal .scb-dd .scb-item')].find(b=>b.innerText.trim()==='Pledged').click()")
            await asyncio.sleep(0.2)
            # B4: Confirm ohne Preis-Zeile.
            await js("document.querySelector('.hform-modal button[type=submit]').click()")
            await asyncio.sleep(0.4)
            conf = await js("(() => { const c=document.querySelector('.advert-scrim .confirm-modal'); return c ? c.innerText : 'NOCONF'; })()")
            check("tb-confirm-opens", "EINTRAG ANLEGEN" in str(conf), str(conf)[:120])
            check("tb-confirm-no-price", "Preis" not in str(conf), str(conf)[:200])
            await shot("03-confirm-no-price.png")
            await js("[...document.querySelectorAll('.confirm-modal button')].find(b=>b.innerText.includes('JA, SPEICHERN')).click()")
            await asyncio.sleep(0.8)
            saved = await js("(() => { const it = window.__HANGAR_DATA__.find(i=>i.name==='Probe Frachter');"
                             " return it ? JSON.stringify({et:it.entity_type, or:it.origin, pr:it.purchase_price}) : 'MISSING'; })()")
            s = json.loads(saved) if saved and str(saved).startswith("{") else {}
            check("tb-saved-with-choice-no-price", s.get("et") == "ship" and s.get("or") == "pledge" and s.get("pr") is None, saved)

            # =============================================================
            # C) Ship-Details: Modulzeilen mit Versicherung/Herkunft
            # =============================================================
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('600i Explorer')).click()")
            await asyncio.sleep(0.5)
            dl = str(await js("(() => { const d=document.querySelector('.hangar-detail'); return d ? d.innerText : 'NODETAIL'; })()"))
            check("tb-detail-no-price", "Preis" not in dl, dl[:200])
            mods0 = await js("(() => { const s=document.querySelector('.hd-mods .h'); return s ? s.innerText : 'NOMODS'; })()")
            check("ship-detail-module-section", "Module (1)" in str(mods0), mods0)
            mrow = await js("(() => { const r=document.querySelector('.hd-mods .hmod'); return r ? r.innerText : 'NOROW'; })()")
            check("tb-module-row-shows-chips", "PLEDGE" in str(mrow) and "LTI" in str(mrow), mrow)
            # C2: Add ohne Herkunft → Meldung.
            await js("(() => { const f=document.querySelector('.hd-mods .hmod-add .rta-input'); if(!f) return 'NOINPUT';"
                     "f.focus(); f.value='Observation'; f.dispatchEvent(new Event('input',{bubbles:true})); return 'OK'; })()")
            await asyncio.sleep(0.4)
            moditems = await js("[...document.querySelectorAll('.hd-mods .hmod-add .rta-dd .rta-item')].map(b=>b.innerText.trim())")
            check("tb-module-autocomplete-file-only", bool(moditems) and any("Observation" in s for s in (moditems or [])), str(moditems))
            if isinstance(moditems, list) and moditems:
                await js("[...document.querySelectorAll('.hd-mods .hmod-add .rta-dd .rta-item')][0].click()")
                await asyncio.sleep(0.3)
            await js("[...document.querySelectorAll('.hmod-add-meta .hbtn')].find(b=>b.innerText.includes('Hinzufügen')).click()")
            await asyncio.sleep(0.4)
            t3 = await toast()
            check("tb-module-herkunft-required", "Herkunft" in str(t3), t3)
            # Modul-Herkunft: KOMPLETTE Liste aufklappen (Task: Pledged / In-Game).
            await js("(() => { const m=document.querySelector('.hmod-add-meta'); m.querySelector('.scb-input').click(); return 'OK'; })()")
            await asyncio.sleep(0.3)
            mopts = await js("[...document.querySelectorAll('.hmod-add-meta .scb-dd .scb-item')].map(b=>b.innerText.trim())")
            check("tb-module-herkunft-options", mopts == ["Pledged", "In-Game"], str(mopts))
            await js("[...document.querySelectorAll('.hmod-add-meta .scb-dd .scb-item')].find(b=>b.innerText.trim()==='Pledged').click()")
            await asyncio.sleep(0.2)
            # Versicherung: LTI wählen (zweites Select in der Meta-Zeile).
            await js("(() => { const s=[...document.querySelectorAll('.hmod-add-meta .scb-input')][1]; s.click(); return 'OK'; })()")
            await asyncio.sleep(0.3)
            await js("[...document.querySelectorAll('.hmod-add-meta .scb-dd .scb-item')].find(b=>b.innerText.trim()==='LTI').click()")
            await asyncio.sleep(0.2)
            await js("[...document.querySelectorAll('.hmod-add-meta .hbtn')].find(b=>b.innerText.includes('Hinzufügen')).click()")
            await asyncio.sleep(0.8)
            mods1 = await js("(() => { const s=document.querySelector('.hd-mods .h'); return s ? s.innerText : 'NOMODS'; })()")
            check("tb-module-added", "Module (2)" in str(mods1), mods1)
            newmod = await js("(() => { const it = window.__HANGAR_DATA__.find(i=>i.entity_type==='module' && i.base_ship==='600i Explorer' && i.id>6);"
                              " return it ? JSON.stringify({n:it.name, or:it.origin, ins:it.insurance}) : 'MISSING'; })()")
            n = json.loads(newmod) if newmod and str(newmod).startswith("{") else {}
            check("tb-module-row-created-ins-origin", n.get("or") == "pledge" and n.get("ins") == "LTI", newmod)
            await shot("04-module-added-ins-origin.png")
            # C3: Klick auf den Modulnamen (das NEUE Modul — letzte Zeile) →
            # Edit-Popup (Typ statisch, ins/origin editierbar).
            await js("[...document.querySelectorAll('.hd-mods .hmod .hmod-name')].pop().click()")
            await asyncio.sleep(0.5)
            etitle = await js("(() => { const l=document.querySelector('.hform-modal .m-lbl'); return l ? l.innerText : 'NOFORM'; })()")
            check("tb-module-edit-opens-popup", etitle == "EINTRAG BEARBEITEN", etitle)
            etyp = await H("const f=[...m.querySelectorAll('.trk-field')].find(x=>x.textContent.includes('Typ'));"
                           "const i=f ? f.querySelector('input') : null;"
                           "return i ? JSON.stringify({v:i.value, dis:i.disabled}) : 'NOFIELD';")
            e = json.loads(etyp) if etyp and str(etyp).startswith("{") else {}
            check("tb-module-edit-typ-static", e.get("v") == "Modul" and e.get("dis") is True, etyp)
            # WICHTIG: das SelectCombo filtert die Dropdown-Liste auf den
            # aktuellen Feldtext — zum Umwählen erst LEEREN, dann picken.
            await js(field_js("Herkunft", "const i=f.querySelector('.scb-input'); i.focus(); i.value='';"
                              "i.dispatchEvent(new Event('input',{bubbles:true})); return 'OK';"))
            await asyncio.sleep(0.3)
            await js("[...document.querySelectorAll('.hform-modal .scb-dd .scb-item')].find(b=>b.innerText.trim()==='In-Game').click()")
            await asyncio.sleep(0.2)
            await js(field_js("Versicherung", "const i=f.querySelector('.scb-input'); i.focus(); i.value='';"
                              "i.dispatchEvent(new Event('input',{bubbles:true})); return 'OK';"))
            await asyncio.sleep(0.3)
            await js("[...document.querySelectorAll('.hform-modal .scb-dd .scb-item')].find(b=>b.innerText.trim()==='3 Monate').click()")
            await asyncio.sleep(0.2)
            await js("document.querySelector('.hform-modal button[type=submit]').click()")
            await asyncio.sleep(0.4)
            await js("[...document.querySelectorAll('.confirm-modal button')].find(b=>b.innerText.includes('JA, SPEICHERN')).click()")
            await asyncio.sleep(0.8)
            ed = await js("(() => { const it = window.__HANGAR_DATA__.find(i=>i.entity_type==='module' && i.base_ship==='600i Explorer' && i.id>6);"
                          " return it ? JSON.stringify({or:it.origin, ins:it.insurance}) : 'MISSING'; })()")
            edj = json.loads(ed) if ed and str(ed).startswith("{") else {}
            check("tb-module-edit-saves-ins-origin", edj.get("or") == "ingame" and edj.get("ins") == "3 Monate", ed)
            await shot("05-module-edited.png")

            # =============================================================
            # D) Module-Filter zeigt Schiffe MIT Modulen
            # =============================================================
            await key("Escape")  # zurück zur Liste
            await asyncio.sleep(0.3)
            await js("[...document.querySelectorAll('.hangar-filters .hchip')].find(b=>b.innerText.includes('MODULE')).click()")
            await asyncio.sleep(0.4)
            frows = await js("[...document.querySelectorAll('.hangar-list .hrow .hname')].map(b=>b.innerText.trim())")
            check("tb-module-filter-shows-ships-with-modules", frows == ["600i Explorer"], str(frows))
            await js("[...document.querySelectorAll('.hangar-filters .hchip')].find(b=>b.innerText.includes('ALLE')).click()")
            await asyncio.sleep(0.3)

            # =============================================================
            # E) Listing-Formular: Auswahl-Felder starten LEER
            # =============================================================
            await js("document.querySelector('input.q').focus(); 'OK'")
            await key("F2")
            await asyncio.sleep(1.0)
            await js("[...document.querySelectorAll('.chip')].find(x=>x.textContent.trim()==='Mein Shop').click(); 'OK'")
            await asyncio.sleep(1.5)
            await js("[...document.querySelectorAll('.chip')].find(x=>x.textContent.trim()==='Anlegen').click(); 'OK'")
            await asyncio.sleep(0.8)
            def scb_state(label):
                return js("(() => { const b=[...document.querySelectorAll('.advert-scrim .mbar2')]"
                          ".find(x=>x.textContent.includes('" + label + "')); if(!b) return 'NOBAR';"
                          "const i=b.querySelector('.scb-input'); if(!i) return 'NOSCB';"
                          "return JSON.stringify({v:i.value, ph:i.placeholder||''}); })()")
            def sjson(v):
                return json.loads(v) if isinstance(v, str) and v.startswith("{") else {}
            lt = sjson(await scb_state("Listing Type"))
            check("tb-listing-type-empty", lt.get("v") == "" and "wählen" in lt.get("ph", ""), lt)
            cur = sjson(await scb_state("Währung"))
            check("tb-listing-currency-empty", cur.get("v") == "" and "Währung" in cur.get("ph", ""), cur)
            un = sjson(await scb_state("Einheit"))
            check("tb-listing-unit-empty", un.get("v") == "" and "Einheit" in un.get("ph", ""), un)
            hr = sjson(await scb_state("Laufzeit"))
            check("tb-listing-hours-empty", hr.get("v") == "" and "Laufzeit" in hr.get("ph", ""), hr)
            await shot("06-listing-form-empty.png")
            # Komplette Optionsliste ohne Tippen: Währung.
            await js("(() => { const b=[...document.querySelectorAll('.advert-scrim .mbar2')]"
                     ".find(x=>x.textContent.includes('Währung')); b.querySelector('.scb-input').click(); return 'OK'; })()")
            await asyncio.sleep(0.3)
            curopts = await js("[...document.querySelectorAll('.advert-scrim .scb-dd .scb-item')].map(b=>b.innerText.trim())")
            check("tb-listing-currency-full-options", curopts == ["UEC", "WIF", "MGS"], str(curopts))
            # Komplette Optionsliste ohne Tippen: Laufzeit (im eigenen mbar2 —
            # das Währungs-Dropdown vorher schließen).
            await js("(() => { const b=[...document.querySelectorAll('.advert-scrim .mbar2')]"
                     ".find(x=>x.textContent.includes('Laufzeit')); const i=b.querySelector('.scb-input'); i.focus(); return 'OK'; })()")
            await asyncio.sleep(0.3)
            hropts = await js("(() => { const b=[...document.querySelectorAll('.advert-scrim .mbar2')]"
                              ".find(x=>x.textContent.includes('Laufzeit'));"
                              "return [...b.querySelectorAll('.scb-dd .scb-item')].map(x=>x.innerText.trim()); })()")
            check("tb-listing-hours-full-options", isinstance(hropts, list) and len(hropts) == 12, str(hropts))
            await shot("07-listing-full-options.png")
            # Popup schließen — dirty (Fokus-Klicks) → Verwerfen bestätigen.
            await js("[...document.querySelectorAll('.advert-scrim button, .advert-scrim .btn')]"
                     ".find(b=>b.innerText.trim()==='Abbrechen')?.click(); 'OK'")
            await asyncio.sleep(0.4)
            await js("[...document.querySelectorAll('.advert-scrim button')].find(b=>b.innerText.trim()==='Verwerfen')?.click(); 'OK'")
            await asyncio.sleep(0.4)
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
