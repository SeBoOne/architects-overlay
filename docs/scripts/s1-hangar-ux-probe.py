#!/usr/bin/env python3
"""CDP-Probe für t_38f79fff (S1: Hangar-UX — Modal-Toast, stabile Liste,
Popup-Größe + Module-Autocomplete nie abgeschnitten).

Datenquelle: synthetischer Hangar-Stand über window.__HANGAR_DATA__ (Stub,
wie r2-hangar-rework-probe) — IPC-Grenze gestubbt, echte Svelte-UI läuft.

Checks:
  1. F11 → Hangar-List (8 Rows).
  2. + Anlegen → Popup offen UND die Liste bleibt UNVERÄNDERT hinter dem
     Scrim stehen (.hangar-list .hrow weiter 8 — kein DOM-Entfernen/Leeren),
     Shell wächst (class shell-advert) statt zu schrumpfen.
  3. KEIN Inline-Text mehr über der Liste (.hangar-note existiert nicht).
  4. Meldung als Modal-Toast im VORDERGRUND: Speichern ohne Name →
     .hangar-toast mit 'Name fehlt', z-index über dem Formular-Scrim (260>150).
  5. Popup groß genug: volle nutzbare Höhe, Speichern-Button sichtbar.
  6. Module-/Name-Autocomplete: .rta-dd vollständig INNERHALB des nächsten
     Scroll-Containers (.hform-body bzw. Shell) — nie am Rand abgeschnitten.

Screenshots: docs/screenshots/hangar-ux/01…05-*.png
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/hangar-ux")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

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
    mk(7, "Constellation Andromeda", "ingame", None, "ship", manufacturer="RSI", custom_name="Bogenschütze"),
    mk(8, "Test Modul", "ingame", None, "module"),
]
N_ROWS = len(ITEMS)

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
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-hangar-s1",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9234/json/list", timeout=1) as r:
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

            async def rows():
                return await js("document.querySelectorAll('.hangar-list .hrow').length")

            # ---- 1: F11 → Hangar-List ----
            await key("F11")
            await asyncio.sleep(1.0)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'") or "").strip()
            check("f11-enters-hangar", smode == "HANGAR", smode)
            check(f"list-{N_ROWS}-rows", (await rows()) == N_ROWS, "rows=" + str(await rows()))
            await shot("01-hangar-list.png")

            # ---- 2: Popup öffnen — Liste bleibt UNVERÄNDERT hinter dem Scrim ----
            await js("[...document.querySelectorAll('.hangar-actions .hbtn')].find(b=>b.innerText.includes('Anlegen')).click()")
            await asyncio.sleep(0.5)
            check("popup-modal-open", (await js("document.querySelector('.advert-scrim .hform-modal') ? 'yes' : 'no'")) == "yes")
            check("list-unchanged-behind-scrim", (await rows()) == N_ROWS, "rows=" + str(await rows()))
            check("shell-grows-not-shrinks",
                  (await js("document.querySelector('.shell').classList.contains('shell-advert')")) == True)
            check("no-inline-note-element",
                  (await js("document.querySelector('.hangar-note') ? 'present' : 'absent'")) == "absent")
            await shot("02-popup-list-behind.png")

            # ---- 3: Meldung als Modal-Toast im Vordergrund ----
            # Speichern ohne Name → 'Name fehlt' als Toast, NICHT inline über der Liste.
            await js("document.querySelector('.hform-modal button[type=submit]').click()")
            await asyncio.sleep(0.4)
            toast = await js("(() => { const t = document.querySelector('.hangar-toast'); return t ? t.innerText.trim() : 'NOTOAST'; })()")
            check("toast-renders-message", "Name fehlt" in str(toast), toast)
            zt = await js("(() => { const t=document.querySelector('.hangar-toast'); const s=document.querySelector('.advert-scrim');"
                          " if (!t || !s) return 'MISSING';"
                          " const rt=t.getBoundingClientRect();"
                          " return JSON.stringify({zT:+getComputedStyle(t).zIndex, zS:+getComputedStyle(s).zIndex,"
                          " top:rt.top>=0, inVp:rt.bottom<=window.innerHeight}); })()")
            z = json.loads(zt) if str(zt).startswith("{") else {}
            check("toast-z-above-form-scrim", z.get("zT", 0) > z.get("zS", 999), zt)
            check("toast-fully-visible", z.get("top") and z.get("inVp"), zt)
            await shot("03-toast-over-popup.png")
            # Toast auto-fade (~3.5 s) bleibt über hangarNoteSet — hier manuell schließen.
            await js("document.querySelector('.hangar-toast .ht-x').click()")
            await asyncio.sleep(0.2)
            check("toast-closable", (await js("document.querySelector('.hangar-toast') ? 'open' : 'closed'")) == "closed")

            # ---- 4: Popup groß genug (volle Höhe, Buttons sichtbar) ----
            geo = await js("""
                const m = document.querySelector('.hform-modal');
                const r = m.getBoundingClientRect();
                JSON.stringify({top: r.top, bottom: r.bottom, left: r.left, right: r.right,
                                vh: window.innerHeight, vw: window.innerWidth,
                                btn: !!m.querySelector('.hform-foot button[type=submit]')})""")
            g = json.loads(geo)
            check("popup-inside-overlay", g["top"] >= 0 and g["left"] >= 0 and g["bottom"] <= g["vh"] and g["right"] <= g["vw"], geo)
            check("popup-save-button-visible", g["btn"] and g["bottom"] <= g["vh"], geo)
            await shot("04-popup-full-height.png")

            # ---- 5: Name-Autocomplete nie am Rand abgeschnitten ----
            await js("(() => { const i = document.querySelector('.hform-modal .rta-input'); i.focus(); i.value='cut';"
                     "i.dispatchEvent(new Event('input',{bubbles:true})); return 'OK'; })()")
            await asyncio.sleep(0.4)
            clip = await js("""
                (() => { try {
                const dd = document.querySelector('.hform-modal .rta-dd');
                if (!dd) return 'NODD';
                const sc = (() => { let p = dd.parentElement;
                    while (p && p !== document.body) {
                        const cs = getComputedStyle(p);
                        if (/(auto|scroll)/.test(cs.overflowY)) return p;
                        p = p.parentElement; } return document.body; })();
                const d = dd.getBoundingClientRect(), s = sc.getBoundingClientRect();
                return JSON.stringify({ddTop: Math.round(d.top), ddBottom: Math.round(d.bottom),
                                       scTop: Math.round(s.top), scBottom: Math.round(s.bottom),
                                       up: dd.classList.contains('is-up'),
                                       items: dd.querySelectorAll('.rta-item').length});
                } catch (e) { return 'ERR:' + (e && e.message || String(e)); } })()""")
            c = json.loads(clip) if str(clip).startswith("{") else {}
            check("typeahead-suggestions-open", c.get("items", 0) > 0, clip)
            check("typeahead-dd-inside-scroll-container",
                  c and c["ddTop"] >= c["scTop"] - 1 and c["ddBottom"] <= c["scBottom"] + 1, clip)
            await shot("05-typeahead-not-clipped.png")

            # ---- 6: Detail-View — Modul-Autocomplete ebenfalls nicht geclippt ----
            await js("(() => { const b = document.querySelector('.hform-modal .hform-foot button:nth-child(2)'); if (b) b.click(); return 'OK'; })()")
            await asyncio.sleep(0.3)
            await js("[...document.querySelectorAll('.hangar-list .hrow')].find(b=>b.innerText.includes('600i Explorer')).click()")
            await asyncio.sleep(0.5)
            await js("(() => { const f=document.querySelector('.hd-mods .hmod-add .rta-input'); f.focus(); f.value='Exploration';"
                     "f.dispatchEvent(new Event('input',{bubbles:true})); return 'OK'; })()")
            await asyncio.sleep(0.4)
            clip2 = await js("""
                (() => { try {
                const dd = document.querySelector('.hd-mods .hmod-add .rta-dd');
                if (!dd) return 'NODD';
                const d = dd.getBoundingClientRect();
                return JSON.stringify({ddTop: Math.round(d.top), ddBottom: Math.round(d.bottom),
                                       vh: window.innerHeight, items: dd.querySelectorAll('.rta-item').length});
                } catch (e) { return 'ERR:' + (e && e.message || String(e)); } })()""")
            c2 = json.loads(clip2) if str(clip2).startswith("{") else {}
            check("detail-module-suggestions-open", c2.get("items", 0) > 0, clip2)
            check("detail-module-dd-fully-visible",
                  c2 and c2["ddTop"] >= 0 and c2["ddBottom"] <= c2["vh"], clip2)
            await shot("06-detail-module-dd.png")
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
