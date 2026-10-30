#!/usr/bin/env python3
"""CDP-Probe für t_f269f5b7 (TC: Hangar-Schiffsdetails als Popup statt eigener Seite).

Datenquelle: synthetischer Hangar-Stand über window.__HANGAR_DATA__ (Stub).
Nur die IPC-Grenze ist gestubbt (smoke-stub.js), die echte Svelte-UI läuft
unverändert.

Checks:
  A) Liste IMMER sichtbar (keine eigene Detail-Seite mehr).
  B) Detail-POPUP (Klick auf Zeile):
     B1 .advert-scrim > .hdetail-modal offen, ~90 % Breite + volle Höhe
        minus Scrim-Rand (nichts eingeengt),
     B2 Liste dahinter UNVERÄNDERT (gleiche Zeilen im DOM),
     B3 Detail-Felder (Modell/Hersteller/Typ/Custom/Datum) vollständig.
  C) Esc schließt das Popup — zurück zur Liste ohne View-Wechsel (HANGAR).
  D) Enter auf Listenzeile öffnet das Popup (Tastatur); ✕-Close-Button
     schließt ebenfalls.
  E) Modul-Bereich im Popup (Schiff mit module-overrides-Eintrag): Chips
     Versicherung/Herkunft je Modulzeile + Add-Zeile.
  F) Bearbeiten öffnet das Formular-Popup ÜBER dem Detail-Popup.
  G) Löschen: 2-Schritt-Bestätigung → Zeile weg, Popup zu, Liste aktualisiert.
  H) Unverbundenes Modul: Popup zeigt "Modul (unverbunden)" + Zuordnen-Zeile.

Screenshots: docs/screenshots/hangar-detail-popup/01…06-*.png
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/hangar-detail-popup")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail: object = ""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + str(detail)) if detail and not ok else ""))

def mk(id, name, origin, insurance, entity_type, base_ship="", manufacturer="",
       date="", custom_name=""):
    return {"id": id, "name": name, "origin": origin, "insurance": insurance,
            "custom_name": custom_name, "entity_type": entity_type,
            "base_ship": base_ship, "ship_code": "", "manufacturer": manufacturer,
            "purchase_price": None, "purchase_date": date, "pledge_id": "",
            "pledge_name": "", "warbond": False, "ccud": False,
            "created_at": None, "updated_at": None}

ITEMS = [
    mk(1, "Cutlass Black", "pledge", "LTI", "ship", manufacturer="Drake Interplanetary", date="2024-07-12", custom_name="SEBO"),
    mk(2, "600i Explorer", "pledge", "LTI", "ship", manufacturer="Origin Jumpworks", date="2024-07-12"),
    mk(3, "Mule", "ingame", None, "vehicle", manufacturer="Drake"),
    # Verlinkt → kein Listen-Eintrag, nur Ship-Details.
    mk(4, "600i Exploration Module", "pledge", "LTI", "module", base_ship="600i Explorer", date="2024-07-12"),
    # Bezugslos → sichtbar („Modul (unverbunden)“).
    mk(5, "Golem OX Modul", "pledge", None, "module"),
]

FOR_SHIP = {
    "600i explorer": {
        "known": True, "has_ports": True,
        "modules": [{"name": "600i Exploration Module", "category": "Utility"}],
    }
}
MODUL_STATUS = {
    "version": "module-overrides.json", "synced_at": 1760000000,
    "age_seconds": 5400, "module_count": 1, "ship_count": 1,
}

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
        "--remote-debugging-port=9236", "--user-data-dir=/tmp/cdp-hangar-tc",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9236/json/list", timeout=1) as r:
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

            # Popup-Zustand kompakt lesen.
            POP = ("(() => { const m = document.querySelector('.advert-scrim .hdetail-modal');"
                   " if (!m) return 'NOMODAL';"
                   " const sc = m.closest('.advert-scrim');"
                   " const mr = m.getBoundingClientRect(), sr = sc.getBoundingClientRect();"
                   " return JSON.stringify({w: mr.width, h: mr.height, sw: sr.width, sh: sr.height,"
                   " name: (m.querySelector('.hd-name')||{}).textContent || '',"
                   " head: (m.querySelector('.hd-head')||{}).innerText,"
                   " body: (m.querySelector('.hangar-detail')||{}).innerText}); })()")
            def rows():
                return js("[...document.querySelectorAll('.hangar-list .hrow')].map(b=>b.innerText)")

            # =============================================================
            # A) Liste IMMER sichtbar
            # =============================================================
            await key("F11")
            await asyncio.sleep(1.0)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'") or "").strip()
            check("f11-enters-hangar", smode == "HANGAR", smode)
            r0 = await rows() or []
            expect_rows = 4  # Cutlass, 600i, Mule, unverbundenes Modul
            check("a-list-initial", len(r0) == expect_rows, f"rows={len(r0)} expect={expect_rows}")

            # =============================================================
            # B) Klick auf Zeile → Detail-POPUP
            # =============================================================
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('Cutlass'))?.click()")
            await asyncio.sleep(0.6)
            p = json.loads(await js(POP)) if not (await js(POP)).startswith("EXC") and (await js(POP)) != "NOMODAL" else None
            if p is None:
                check("b1-popup-opens-on-click", False, await js(POP))
            else:
                check("b1-popup-opens-on-click", True)
                # Breite ~90 % des Scrims, Höhe = volle Höhe minus Scrim-Rand.
                frac = p["w"] / p["sw"] if p["sw"] else 0
                check("b2-popup-width-90pct", 0.85 <= frac <= 0.95, f"w={p['w']:.0f} scrim={p['sw']:.0f} frac={frac:.3f}")
                check("b2-popup-full-height", p["h"] <= p["sh"] - 20 and p["h"] >= p["sh"] - 40, f"h={p['h']:.0f} scrim={p['sh']:.0f}")
                # Liste dahinter unverändert.
                r1 = await rows() or []
                check("b3-list-unchanged-behind", r1 == r0, f"rows_before={len(r0)} rows_after={len(r1)}")
                # Detail-Felder vollständig.
                body = p["body"]
                for lbl in ["Modell", "Hersteller", "Typ", "Custom-Name", "Datum"]:
                    check("b3-detail-field-" + lbl.lower().replace("-", ""), lbl in body, body[:120])
                check("b3-detail-values", "Cutlass Black" in body and "Drake" in body and "SEBO" in body, body[:200])
            await shot("01-popup-open-cutlass.png")

            # =============================================================
            # C) Esc schließt das Popup — Liste bleibt, kein View-Wechsel
            # =============================================================
            await key("Escape")
            await asyncio.sleep(0.4)
            gone = await js("document.querySelector('.advert-scrim .hdetail-modal') ? 'OPEN' : 'CLOSED'")
            smode2 = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'") or "").strip()
            r2 = await rows() or []
            check("c1-esc-closes-popup", gone == "CLOSED", gone)
            check("c2-still-hangar-after-esc", smode2 == "HANGAR", smode2)
            check("c3-list-intact-after-esc", r2 == r0, f"rows={len(r2)}")

            # =============================================================
            # D) Enter öffnet das Popup (Tastatur), ✕ schließt
            # =============================================================
            await key("ArrowDown")
            await key("Enter")
            await asyncio.sleep(0.5)
            p2 = await js(POP)
            check("d1-enter-opens-popup", p2 != "NOMODAL" and not p2.startswith("EXC"), p2[:120])
            await shot("02-popup-open-enter.png")
            closed_by_x = await js("(() => { const x = document.querySelector('.hdetail-modal .hd-close'); if (!x) return 'NOX'; x.click(); return 'CLICKED'; })()")
            await asyncio.sleep(0.4)
            gone2 = await js("document.querySelector('.advert-scrim .hdetail-modal') ? 'OPEN' : 'CLOSED'")
            check("d2-close-button-works", closed_by_x == "CLICKED" and gone2 == "CLOSED", f"x={closed_by_x} state={gone2}")

            # =============================================================
            # E) Modul-Bereich im Popup (600i Explorer: overrides-Eintrag)
            # =============================================================
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('600i'))?.click()")
            await asyncio.sleep(0.8)
            p3raw = await js(POP)
            if p3raw == "NOMODAL" or p3raw.startswith("EXC"):
                check("e1-600i-popup", False, p3raw[:120])
            else:
                p3 = json.loads(p3raw)
                mods = p3["body"]
                check("e1-modules-section", "Module (1)" in mods, mods[:200])
                check("e2-module-row-chips", "600i Exploration Module" in mods and "LTI" in mods, mods[:200])
                has_add = await js("document.querySelector('.hdetail-modal .hmod-add .rta-input') ? 'yes' : 'no'")
                has_meta = await js("document.querySelector('.hdetail-modal .hmod-add-meta') ? 'yes' : 'no'")
                check("e3-module-add-row", has_add == "yes" and has_meta == "yes", f"add={has_add} meta={has_meta}")
            await shot("03-popup-modules-600i.png")
            await key("Escape")
            await asyncio.sleep(0.4)

            # =============================================================
            # F) Bearbeiten öffnet Formular-Popup ÜBER dem Detail-Popup
            # =============================================================
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('Cutlass'))?.click()")
            await asyncio.sleep(0.5)
            await js("[...document.querySelectorAll('.hdetail-modal .hbtn')].find(b=>b.innerText.includes('Bearbeiten'))?.click()")
            await asyncio.sleep(0.5)
            both = await js("(() => ({ detail: document.querySelector('.advert-scrim .hdetail-modal') ? 'yes' : 'no', form: document.querySelector('.advert-scrim .hform-modal') ? 'yes' : 'no' }))()")
            check("f1-edit-form-over-detail", isinstance(both, dict) and both.get("detail") == "yes" and both.get("form") == "yes", both)
            await shot("04-popup-edit-over-detail.png")
            await key("Escape")  # Formular zu
            await asyncio.sleep(0.4)
            detail_still = await js("document.querySelector('.advert-scrim .hdetail-modal') ? 'yes' : 'no'")
            check("f2-esc-closes-form-keeps-detail", detail_still == "yes", detail_still)
            await key("Escape")  # Detail zu
            await asyncio.sleep(0.4)

            # =============================================================
            # G) Löschen im Popup (2-Schritt), Liste aktualisiert
            # =============================================================
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('Mule'))?.click()")
            await asyncio.sleep(0.5)
            await js("[...document.querySelectorAll('.hdetail-modal .hbtn')].find(b=>b.innerText.includes('Löschen'))?.click()")
            await asyncio.sleep(0.5)
            confirm_open = await js("document.querySelector('.confirm-modal, .c-modal, .hangar-confirm') ? 'yes' : 'no'")
            # Confirm-Modal Klasse herausfinden (2-Schritt-Bestätigung).
            confirm_any = await js("document.body.innerText.includes('JA, LÖSCHEN') ? 'yes' : 'no'")
            check("g1-confirm-2step-opens", confirm_any == "yes", f"modal={confirm_open} text={confirm_any}")
            await shot("05-popup-delete-confirm.png")
            await js("[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='JA, LÖSCHEN')?.click()")
            await asyncio.sleep(0.8)
            after = await rows() or []
            popup_after = await js("document.querySelector('.advert-scrim .hdetail-modal') ? 'OPEN' : 'CLOSED'")
            check("g2-delete-closes-popup", popup_after == "CLOSED", popup_after)
            check("g3-delete-removes-row", not any("Mule" in t for t in after), f"rows={after}")
            smode3 = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'") or "").strip()
            check("g4-still-hangar-after-delete", smode3 == "HANGAR", smode3)

            # =============================================================
            # H) Unverbundenes Modul: Popup mit Zuordnen-Zeile
            # =============================================================
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('Golem'))?.click()")
            await asyncio.sleep(0.5)
            ph = await js(POP)
            if ph == "NOMODAL" or ph.startswith("EXC"):
                check("h1-unbound-popup", False, ph[:120])
            else:
                p = json.loads(ph)
                check("h1-unbound-popup", "unverbunden" in p["head"] or "unverbunden" in p["body"], p["head"] + " | " + p["body"][:160])
                assign = await js("document.querySelector('.hdetail-modal .hmod-add .rta-input') ? 'yes' : 'no'")
                check("h2-assign-row", assign == "yes", assign)
            await shot("06-popup-unbound-module.png")
            await key("Escape")
            await asyncio.sleep(0.3)

    finally:
        server.terminate()
        proc.terminate()

    print(f"\n== {len(PASS)} PASS / {len(FAIL)} FAIL ==")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
    raise SystemExit(1 if FAIL else 0)

asyncio.run(main())
