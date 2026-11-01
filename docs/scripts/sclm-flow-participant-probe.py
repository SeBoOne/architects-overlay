#!/usr/bin/env python3
"""CDP-Probe für t_bf2374f7 (SF2: SCLM-Teilnehmer-Flow komplett).

Datenquelle: STUBS (smoke-stub.js — __STUB_CONNECTORS__ + __STUB_CONNECTOR_DATA__).
Die echte Svelte-UI läuft unverändert; die IPC-Grenze ist gestubbt. Verifiziert
den kompletten Teilnehmer-Flow exakt nach Sebo-Ablauf:

  1. Suchleisten-Übernahme: im own-Modus (F7) ist die Suchleiste das Code-
     Eingabefeld (Platzhalter code_ph); Enter → connector_fetch(viewId 'code')
     → Session öffnet DIREKT im participants-Screen (on_submit-Transition).
  2. Flow-Tab 'Teilnehmer' → code_entry-Screen rendert Params als Eingabefelder.
  3. participants (Teilnehmerliste): Zeilen (Sebo/Anders), Add-Self-Action.
  4. addme-Formular: Submit seedet sid/code aus dem Flow-Zustand.
  5. wishes (Multi-Select): 2 von 3 markieren (max_wishes=3) → Confirm ruft
     connector_screen_action mit loot_id-Array → goto wait.
  6. wait: Poll-Screen bleibt (raffle_done=0), nach Zustands-Wechsel auf 1
     (Bool-Toleranz 1==true!) feuert die Exit-Bedingung → results.
  7. results: eigener Gewinn gerendert; collect-Action ruft
     connector_screen_action mit rid.
Screenshots: 01-flow-searchbar-entry, 02-flow-code-entry, 03-flow-participants,
04-flow-addme, 05-flow-wishes, 06-flow-wait, 07-flow-results.

(Vorgänger-Version mit Wishlist-Flow: sclm-flow-participant-probe-v1.py.bak)
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
MANIFEST = json.loads((ROOT / "examples/sclm.json").read_text())
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/sclm-flow-participant")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

SCLM = {"id": "sclm", "enabled": True, "auth_set": {}, "hotkey": "F7", "manifest": MANIFEST}
DATA = {
    "code": {"mapped": {"sid": "s1", "name": "Weihnachtsverlosung", "created": "01.11.2026",
                         "max_wishes": 3, "locked": False, "raffle_done": 0}},
    "participants": {"rows": [
        {"name": "Sebo", "participant_id": "p1"},
        {"name": "Anders", "participant_id": "p2"}]},
    "addme": {"mapped": {}},
    "wishes": {"rows": [
        {"name": "Cutlass Black", "loot_id": "L1", "qty": 1},
        {"name": "Caterpillar", "loot_id": "L2", "qty": 1},
        {"name": "Freelancer", "loot_id": "L3", "qty": 1}]},
    "wait": {"mapped": {"raffle_done": 0, "name": "Weihnachtsverlosung"}},
    "results": {"mapped": {"rid": "R1", "won": ["Cutlass Black"],
                            "waived": False, "collected": False}},
}

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8797", "--bind", "127.0.0.1",
        "--directory", str(ROOT / "build"),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    for _ in range(60):
        try:
            with urllib.request.urlopen(URL, timeout=1) as r:
                if r.status == 200: break
        except Exception: pass
        await asyncio.sleep(0.25)
    else:
        raise RuntimeError("static server did not come up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-sclm-flow",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9234/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception: pass
        await asyncio.sleep(0.25)
    if ws is None:
        server.terminate(); proc.terminate()
        raise RuntimeError("chromium CDP did not come up")

    try:
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
                    msg = json.loads(raw)
                    if "id" in msg and msg["id"] in pending:
                        pending[msg["id"]].set_result(msg)
            asyncio.create_task(recv_loop())
            def ok(resp):
                if "error" in resp: raise RuntimeError("CDP error: %s" % resp["error"])
                return resp.get("result", {})

            await call("Page.enable"); await call("Runtime.enable")
            await call("Emulation.setDeviceMetricsOverride",
                       {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
            boot = ("try { for (const k of Object.keys(localStorage)) if (k.startsWith('connector.flow.pin')) localStorage.removeItem(k); } catch (e) {}\n"
                    + STUB
                    + "\nwindow.__STUB_CONNECTORS__ = " + json.dumps([SCLM]) + ";"
                    + "\nwindow.__STUB_CONNECTOR_DATA__ = " + json.dumps(DATA) + ";"
                    + "\nwindow.__STUB_CONNECTOR_FORM_BODY__ = " + json.dumps({"id": "p1", "name": "Sebo", "pcode": "PC1"}) + ";")
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": boot})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate",
                                  {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + json.dumps(r["exceptionDetails"].get("exception", {}).get("description", ""))[:300]
                return r.get("result", {}).get("value")

            async def key(k, vk):
                await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                      "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
                await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                      "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
                await asyncio.sleep(0.4)

            async def shot(name):
                r = ok(await call("Page.captureScreenshot", {"format": "png"}))
                (OUT / name).write_bytes(base64.b64decode(r["data"]))
                print("  shot:", OUT / name)

            # ---- 1) Suchleisten-Übernahme: F7 → own-Modus, Platzhalter = Code-Feld
            await key("F7", 118)
            ph = await js("document.querySelector('.q') ? document.querySelector('.q').placeholder : 'NONE'")
            check("searchbar-is-code-field", ("Code eingeben" in (ph or "")) or ("Enter code" in (ph or "")), str(ph))
            # Code eintippen → Enter → Session öffnen (direkt Teilnehmerliste)
            await js("var q=document.querySelector('.q'); q.focus();"
                     "q.value='AB12CD'; q.dispatchEvent(new Event('input',{bubbles:true}));")
            await key("Enter", 13); await asyncio.sleep(0.8)
            last = await js("window.__STUB_LAST_FETCH__ ? JSON.stringify(window.__STUB_LAST_FETCH__) : null")
            check("searchbar-enter-fetches-code-screen", last is not None and "'code'" in str(last).replace('"', "'") and "AB12CD" in str(last), str(last))
            rows_txt = await js("document.querySelector('.flist') ? document.querySelector('.flist').innerText : ''")
            check("searchbar-opens-participants-directly", "Sebo" in (rows_txt or "") and "Anders" in (rows_txt or ""), (rows_txt or "")[:120])
            pins = await js("localStorage.getItem('connector.flow.pin.sclm.participant')")
            check("flow-pins-saved-from-searchbar", pins is not None and "AB12CD" in (pins or "") and "s1" in (pins or ""), str(pins))
            act = await js("document.querySelector('.factions') ? document.querySelector('.factions').innerText : ''")
            check("participants-has-add-self-action", ("Selbst hinzufügen" in (act or "")) or ("Add myself" in (act or "")), str(act))
            await shot("01-flow-searchbar-entry.png")

            # Flow verlassen (ESC-Kaskade) → zurück in den own-Modus
            await js("[...document.querySelectorAll('.fbtn')].find(b=>/esc/i.test(b.innerText))?.click()")
            await asyncio.sleep(0.5)
            dbg = await js("JSON.stringify({mode:document.querySelector('.smode')?.innerText, fhead:!!document.querySelector('.fhead'), tabs:[...document.querySelectorAll('.connector .ctab')].map(b=>b.innerText)})")
            print("  DBG after ESC:", dbg)

            # ---- 2) Flow-Tab → code_entry rendert Params als Eingabefelder
            await key("F7", 118)
            await js("[...document.querySelectorAll('.connector .ctab')].find(b=>/^(Teilnehmer|Participant)$/.test(b.innerText.trim()))?.click()")
            await asyncio.sleep(0.6)
            fin = await js("document.querySelectorAll('.fform .finput').length")
            check("code-entry-renders-param-input", fin == 1, "inputs=" + str(fin))
            await shot("02-flow-code-entry.png")
            # Code-Submit im Flow-Renderer selbst → Teilnehmerliste
            await js("var i=document.querySelector('.fform .finput'); i.value='AB12CD';"
                     "i.dispatchEvent(new Event('input',{bubbles:true}));")
            await js("document.querySelector('.fform button[type=submit]').click()")
            await asyncio.sleep(0.7)
            rows_txt = await js("document.querySelector('.flist') ? document.querySelector('.flist').innerText : ''")
            check("code-submit-transitions-to-participants", "Sebo" in (rows_txt or ""), (rows_txt or "")[:120])
            await shot("03-flow-participants.png")

            # ---- 3) Add-Self-Action → addme-Formular (Seeding + Submit)
            await js("[...document.querySelectorAll('.factions .fbtn')].find(b=>/Selbst hinzufügen|Add myself/i.test(b.innerText))?.click()")
            await asyncio.sleep(0.5)
            labels = await js("[...document.querySelectorAll('.fform .flabel')].map(e=>e.innerText)")
            check("addme-form-fields",
                  any(("Session-ID" in (l or "")) or ("Session ID" in (l or "")) for l in (labels or [])),
                  str(labels))
            seeded = await js("JSON.stringify([...document.querySelectorAll('.fform input')].map(i=>i.value))")
            check("addme-seeds-sid-code-from-flow-state",
                  "s1" in (seeded or "") and "AB12CD" in (seeded or ""), seeded)
            await shot("04-flow-addme.png")
            await js("document.querySelector('.fform button[type=submit]').click()")
            await asyncio.sleep(0.6)
            submit = await js("window.__STUB_LAST_SUBMIT__ ? JSON.stringify(window.__STUB_LAST_SUBMIT__) : null")
            check("addme-submit-seeded-values",
                  submit is not None and '"sid":"s1"' in str(submit).replace(' ', '') and '"code":"AB12CD"' in str(submit).replace(' ', ''),
                  str(submit))
            # → wishes (Multi-Liste)

            # ---- 4) Wünsche: multi markieren (2 von max_wishes=3), Confirm
            marks = await js("document.querySelectorAll('.fmark').length")
            check("wishes-renders-multi-checkboxes", marks == 3, "marks=" + str(marks))
            info = await js("document.querySelector('.fmarkinfo') ? document.querySelector('.fmarkinfo').innerText : ''")
            check("wishes-shows-max-counter", "3" in (info or ""), info)
            await js("document.querySelectorAll('.fmark')[0].click(); document.querySelectorAll('.fmark')[1].click();")
            await asyncio.sleep(0.3)
            info2 = await js("document.querySelector('.fmarkinfo') ? document.querySelector('.fmarkinfo').innerText : ''")
            check("wishes-counter-updates", "2" in (info2 or ""), info2)
            await shot("05-flow-wishes.png")
            await js("document.querySelector('.factions .fsubmit')?.click()")
            await asyncio.sleep(0.7)
            action = await js("window.__STUB_LAST_ACTION__ ? JSON.stringify(window.__STUB_LAST_ACTION__) : null")
            ok_action = action is not None and "'wishes'" in str(action).replace('"', "'") and "'confirm'" in str(action).replace('"', "'") \
                and ('"loot_id":["L1","L2"]' in str(action).replace(' ', '')) \
                and ('"participant_id":"p1"' in str(action).replace(' ', ''))
            check("wishes-confirm-posts-loot-array", ok_action, str(action))
            # goto wait
            wait_txt = await js("document.querySelector('.fwait') ? document.querySelector('.fwait').innerText : ''")
            check("wishes-confirm-goto-wait",
                  ("15" in (wait_txt or "")) or ("prüfen" in (wait_txt or "").lower()) or ("check" in (wait_txt or "").lower()),
                  wait_txt)
            await shot("06-flow-wait.png")

            # ---- 5) Ziehung vorbei: raffle_done 1 (SQLite-Int) → results
            await js("window.__STUB_CONNECTOR_DATA__.wait.mapped.raffle_done = 1;")
            # Erst zurück (wait → wishes), dann wieder vor — go() auf denselben
            # Screen lädt nicht neu; der Wait-Poll (15s) wäre zu langsam.
            await js("[...document.querySelectorAll('.fbtn')].find(b=>/^(zurück|back)/i.test(b.innerText.trim()))?.click()")
            await asyncio.sleep(0.4)
            dbg_tab = await js("JSON.stringify([...document.querySelectorAll('.ftab')].map(b=>b.innerText))")
            print("  DBG tabs:", dbg_tab)
            await js("[...document.querySelectorAll('.ftab')].find(b=>/Warten|Awaiting/.test(b.innerText))?.click()")
            await asyncio.sleep(0.7)
            dbg_now = await js("JSON.stringify({step:document.querySelector('.fstep')?.innerText, body:(document.querySelector('.fbody')?.innerText||'').slice(0,120), err:(document.querySelector('.ferror')?.innerText||'')})")
            print("  DBG after wait tab:", dbg_now)
            res_txt = await js("document.querySelector('.fdetail') ? document.querySelector('.fdetail').innerText : ''")
            check("wait-exit-fires-with-sqlite-int", "Cutlass Black" in (res_txt or ""), (res_txt or "")[:200])
            btns = await js("document.querySelector('.factions') ? document.querySelector('.factions').innerText : ''")
            check("results-has-collect-waive-actions",
                  ("abgeholt" in (btns or "").lower() or "collected" in (btns or "").lower())
                  and ("verzicht" in (btns or "").lower() or "waive" in (btns or "").lower()),
                  str(btns))
            await js("[...document.querySelectorAll('.factions .fbtn')].find(b=>/abgeholt|collected/i.test(b.innerText))?.click()")
            await asyncio.sleep(0.6)
            action2 = await js("window.__STUB_LAST_ACTION__ ? JSON.stringify(window.__STUB_LAST_ACTION__) : null")
            ok2 = action2 is not None and "'collect'" in str(action2).replace('"', "'") and '"rid":"R1"' in str(action2).replace(' ', '')
            check("results-collect-action-posts-rid", ok2, str(action2))
            await shot("07-flow-results.png")
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
