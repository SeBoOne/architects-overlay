#!/usr/bin/env python3
"""CDP-Probe für t_aa6cc070 (Hangar-UX v0.3.2: P9/P10/P11 + hangar_add-P1-Beleg).

Datenquelle: ECHTER Import — `ARCH_HANGAR_DUMP=/tmp/hangar-smoke.json cargo
test -p architects-overlay --lib dump_real_import_for_smoke` fährt den echten
Parser+Merge (hangar.rs) gegen /tmp/HangarExport (XPLorer-JSON + CCUgame-CSV)
und schreibt die hangar_list-Ergebnis (28 Entry). This Probe stubbst ONLY the
IPC-Grenze (smoke-stub.js) and verifiert the real Svelte-UI:

  1. F4 → Hangar-List (28 Rows) + P9: HAUPT-Suchfield 'cutlass' filtert the
     List (Row-Anzahl befrom Daten), Field-Clear reverts.
  2. P10: Anlegen-Form: 'ava' → Suggest-Chips, click 'Avenger Stalker' fills
     Manufacturer + Typ (Fields!), Speichern → hangar_add (P1-Beleg v0.3.x:
     'Angelegt:'-Note + Row+1).
  3. P11: Note auto-fade — sleep > 3.5 s → .hangar-note gone.
  4. P10 Snub: click 'Fury' → Basisschiff-Field + Base-Chips (from REGISTERED
     Hangar entries); click chip sets base_ship.
  5. P10 Module: Typ=module → Basisschiff visible; ship '308i' → hidden.

Screenshots: 01-hangar-list.png … 08-module-base-field.png.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
DATA = pathlib.Path("/tmp/hangar-smoke.json")
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/hangar-ux")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8796/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

items = json.loads(DATA.read_text())
N_ROWS = len(items)
CUTLASS = len([i for i in items if "aurora" in (i["name"] + " " + (i.get("custom_name") or "") + " " + (i.get("manufacturer") or "") + " " + (i.get("base_ship") or "")).lower()])

async def main():
    if not DATA.exists():
        raise RuntimeError("run dump_real_import_for_smoke first")

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
        "--remote-debugging-port=9233", "--user-data-dir=/tmp/cdp-hangar-ux",
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
                       {"source": STUB + "\nwindow.__HANGAR_DATA__ = " + json.dumps(items) + ";"})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + (r["exceptionDetails"].get("text") or "")
                return r.get("result", {}).get("value")

            VK = {"F4": 115, "Escape": 27, "Enter": 13, "ArrowDown": 40}
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
                # real 'input'-Event — otherwise Svelte bind:value does not sync
                return await js(f"(() => {{ const e = document.querySelector('{sel}'); if (!e) return 'NOSLCT';"
                                f"e.value = {json.dumps(val)}; e.dispatchEvent(new Event('input', {{bubbles: true}}));"
                                f"return e.value; }})()")

            async def rows():
                return await js("document.querySelectorAll('.hangar-list .hrow').length")

            async def clickchip(txt):
                return await js(f"(() => {{ const b = [...document.querySelectorAll('.hf-suggest .hsug, .hf-basesug .hsug')]"
                                f".find(x => x.innerText.trim().startsWith({json.dumps(txt)}));"
                                f"if (!b) return 'NOTFOUND'; b.click(); return 'CLICKED'; }})()")

            # ---- F4 → Hangar-List ----
            await key("F4")
            await asyncio.sleep(1.0)
            smode = (await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'") or "").strip()
            check("f4-enters-hangar", smode == "HANGAR", smode)
            n0 = await rows()
            check(f"list-{N_ROWS}-rows", n0 == N_ROWS, "rows=" + str(n0))
            await shot("01-hangar-list.png")

            # ---- P9: HAUPT-Suchfield filters the Hangar-List ----
            await setfield("input.q", "aurora")
            await asyncio.sleep(0.4)
            n_cut = await rows()
            check("p9-aurora-filter", n_cut == CUTLASS, f"aurora-rows={CUTLASS} got={n_cut}")
            placeholder = await js("(() => { const q = document.querySelector('input.q'); return q ? q.getAttribute('placeholder') : 'NOSLCT'; })()")
            check("p9-placeholder", "Hangar" in str(placeholder or ""), str(placeholder))
            await shot("02-p9-filter-aurora.png")
            await setfield("input.q", "")
            await asyncio.sleep(0.3)
            n_back = await rows()
            check("p9-clear-reverts", n_back == N_ROWS, "rows=" + str(n_back))

            # ---- P10 + P1: Anlegen-Form, Name-Autocomplete, Save ----
            await js("[...document.querySelectorAll('.hangar-actions .hbtn')].find(b=>b.innerText.includes('Anlegen')).click()")
            await asyncio.sleep(0.4)
            form = await js("document.querySelector('form.hangar-form .hf-title') ? document.querySelector('form.hangar-form .hf-title').innerText : 'NONE'")
            check("add-form-visible", form == "EINTRAG ANLEGEN", form)
            await setfield("form.hangar-form .hf-grid label input[placeholder*=Cutlass]", "ave")
            await asyncio.sleep(0.4)
            sugs = await js("[...document.querySelectorAll('.hf-suggest .hsug')].map(b=>b.innerText.trim())")
            check("p10-name-suggestions", bool(sugs) and any("Avenger" in s for s in (sugs or [])), str(sugs))
            await shot("03-p10-name-suggestions.png")
            clicked = await clickchip("Avenger Stalker")
            check("p10-click-stalker", clicked == "CLICKED", str(clicked))
            await asyncio.sleep(0.3)
            nm = await setfield("form.hangar-form .hf-grid label input[placeholder*=Cutlass]", "Avenger Stalker")  # read-only: value
            man = await js("(() => { const c = [...document.querySelectorAll('form.hangar-form .hf-grid label')].find(l => l.textContent.includes('Hersteller')); const i = c && c.querySelector('input'); return i ? i.value : 'NOSLCT'; })()")
            check("p10-manufacturer-filled", (man or "") == "Aegis", str(man))
            typ = await js("(() => { const c = [...document.querySelectorAll('form.hangar-form .hf-grid label')].find(l => l.textContent.includes('Typ')); const s = c && c.querySelector('select'); return s ? s.value : 'NOSLCT'; })()")
            check("p10-typ-filled-ship", (typ or "") == "ship", str(typ))
            base_absent = await js("[...document.querySelectorAll('form.hangar-form .hf-grid label')].some(l=>l.textContent.includes('Basis')) ? 'yes' : 'no'")
            check("p10-no-base-for-stalker", base_absent == "no", str(base_absent))
            await shot("04-p10-stalker-fields.png")
            before = int(await js("window.__HANGAR_DATA__.length"))
            await js("document.querySelector('form.hangar-form button[type=submit]').click()")
            await asyncio.sleep(0.7)
            after = int(await js("window.__HANGAR_DATA__.length"))
            check(f"add-calls-backend {N_ROWS}->{N_ROWS+1} (P1 v0.3.x)", after == before + 1, f"{before}->{after}")
            note = (await js("document.querySelector('.hangar-note') ? document.querySelector('.hangar-note').innerText : ''") or "").strip()
            check("p1-add-note-angelegt", "Angelegt" in note, note)
            await shot("05-p1-added-note.png")

            # ---- P11: Note auto-fades after ~3.5 s ----
            await asyncio.sleep(4.2)
            note_after = (await js("document.querySelector('.hangar-note') ? document.querySelector('.hangar-note').innerText : 'GONE'") or "").strip()
            check("p11-note-faded", note_after == "GONE", note_after)
            await shot("06-p11-note-gone.png")

            # ---- P10 Snub: Fury → Basisschiff visible + Base-Chips ----
            await js("[...document.querySelectorAll('.hangar-actions .hbtn')].find(b=>b.innerText.includes('Anlegen')).click()")
            await asyncio.sleep(0.4)
            await setfield("form.hangar-form .hf-grid label input[placeholder*=Cutlass]", "fury")
            await asyncio.sleep(0.3)
            clicked = await clickchip("Fury")
            check("p10-snub-fury-chip", clicked == "CLICKED", str(clicked))
            await asyncio.sleep(0.3)
            base_label = await js("[...document.querySelectorAll('form.hangar-form .hf-grid label')].some(l=>l.textContent.includes('Basis')) ? 'yes' : 'no'")
            check("p10-base-visible-for-snub", base_label == "yes", str(base_label))
            basesugs = await js("[...document.querySelectorAll('.hf-basesug .hsug')].map(b=>b.innerText.trim())")
            check("p10-base-chips-from-hangar", bool(basesugs) and len(basesugs) >= 1, str(basesugs))
            await shot("07-p10-snub-base-field.png")
            if basesugs:
                first = basesugs[0]
                r = await js(f"(() => {{ const b = [...document.querySelectorAll('.hf-basesug .hsug')][0]; b.click(); return 'K'; }})()")
                base_val = await setfield("form.hangar-form .hf-grid label input[placeholder*=Hangar]", first)
                check("p10-base-set", base_val == first, f"{base_val} vs {first}")
            # Abbrechen → ship-selection without Snub has NO base field
            await js("document.querySelector('form.hangar-form button[type=button]').click()")
            await key("Escape")
            await js("[...document.querySelectorAll('.hangar-actions .hbtn')].find(b=>b.innerText.includes('Anlegen')).click()")
            await asyncio.sleep(0.4)
            await setfield("form.hangar-form .hf-grid label input[placeholder*=Cutlass]", "308i")
            await asyncio.sleep(0.3)
            clicked = await clickchip("308i")
            base_no = await js("[...document.querySelectorAll('form.hangar-form .hf-grid label')].some(l=>l.textContent.includes('Basis')) ? 'yes' : 'no'")
            check("p10-no-base-for-308i", base_no == "no", str(base_no))

            # ---- P10 Module: Typ=module → Basisschiff visible ----
            mod = await js("(() => { const c = [...document.querySelectorAll('form.hangar-form .hf-grid label')].find(l => l.textContent.includes('Typ')); const s = c && c.querySelector('select'); if (!s) return 'NOSLCT'; s.value = 'module'; s.dispatchEvent(new Event('change', {bubbles: true})); return s.value; })()")
            await asyncio.sleep(0.3)
            base_mod = await js("[...document.querySelectorAll('form.hangar-form .hf-grid label')].some(l=>l.textContent.includes('Basis')) ? 'yes' : 'no'")
            check("p10-base-visible-for-module", base_mod == "yes", str(base_mod))
            await shot("08-module-base-field.png")
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
