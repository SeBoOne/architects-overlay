#!/usr/bin/env python3
"""CDP-Probe für t_bf2374f7 SF2-R2 (Review-D1–D4) — Stub-Harness wie
sclm-flow-participant-probe.py, aber NUR die Review-Defekte:

  D1: Suchleisten-Einstieg (startOwnFlow) → wishes zeigt das echte Limit
      („0 of 3"), 4. Markierung wird geblockt (max_wishes=3).
  D2: Wiedereinstieg mit raffle_done=1 (code-mapped) → Suchleiste → Code →
      Enter landet DIREKT im results-Screen (exit-Bedingung des code-Screens),
      nicht auf der Teilnehmerliste. Pins (pcode/participant_id) vorbereitet
      wie bei einem zurückkehrenden Teilnehmer.
  D3: Select-Pfad („aus Liste wählen", admin-angelegter Teilnehmer ohne
      pcode): Confirm ohne Claim-Code → KEIN Action-Call, param_required-Note;
      mit eingetipptem Claim-Code → connector_screen_action mit pcode,
      Feld wird als Session-Pin gesichert.
  D4: wishes-Zeilen zeigen type/grade/size (SCLM-Chips-Daten) aus dem Mapping.

Screenshots: r2-01 (wishes limit), r2-02 (claim field), r2-03 (re-entry results).
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
MANIFEST = json.loads((ROOT / "examples/sclm.json").read_text())
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/sclm-flow-r2")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8798/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

SCLM = {"id": "sclm", "enabled": True, "auth_set": {}, "hotkey": "F7", "manifest": MANIFEST}
LOOT = [
    {"name": "Cutlass Black", "loot_id": "L1", "qty": 1, "type": "vehicle", "grade": "A", "size": "S"},
    {"name": "Caterpillar", "loot_id": "L2", "qty": 1, "type": "vehicle", "grade": "B", "size": "M"},
    {"name": "Freelancer", "loot_id": "L3", "qty": 1, "type": "vehicle", "grade": "C", "size": "M"},
    {"name": "Hornet", "loot_id": "L4", "qty": 1, "type": "vehicle", "grade": "A", "size": "S"},
]

def data(raffle_done):
    return {
        "code": {"mapped": {"sid": "s1", "name": "Weihnachtsverlosung", "created": "01.11.2026",
                             "max_wishes": 3, "locked": False, "raffle_done": raffle_done}},
        "participants": {"rows": [
            {"name": "Sebo", "participant_id": "p1"},
            {"name": "Anders", "participant_id": "p2"}]},
        "wishes": {"rows": LOOT},
        "results": {"mapped": {"rid": "R1", "won": ["Cutlass Black"],
                                "waived": False, "collected": False}},
    }

SCENARIO = os.environ.get("R2_SCENARIO", "main")

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8798", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9235", "--user-data-dir=/tmp/cdp-sclm-flow-r2",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9235/json/list", timeout=1) as r:
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

            async def boot(data, pins=None):
                boot_n[0] += 1
                mark = f"window.__BOOT_SEQ__=(window.__BOOT_SEQ__||[]);window.__BOOT_SEQ__.push('b{boot_n[0]}');\n"
                clear = ("try { for (const k of Object.keys(localStorage)) "
                         "if (k.startsWith('connector.flow.pin')) localStorage.removeItem(k); } catch (e) {}")
                pin_js = ""
                if pins:
                    pin_js = ("\nlocalStorage.setItem('connector.flow.pin.sclm.participant', "
                              + json.dumps(json.dumps(pins)) + ");")
                return (mark + clear + pin_js + "\n" + STUB
                        + "\nwindow.__STUB_CONNECTORS__ = " + json.dumps([SCLM]) + ";"
                        + "\nwindow.__STUB_CONNECTOR_DATA__ = " + json.dumps(data) + ";")

            boot_n = [0]
            async def navigate(boot_src):
                await call("Page.addScriptToEvaluateOnNewDocument", {"source": boot_src})
                await call("Page.navigate", {"url": URL})
                await asyncio.sleep(2.5)
                await key("F7", 118)

            if SCENARIO == "main":
              # ============ D1: Suchleisten-Pfad → Limit + D4: Chips ============
              await navigate(await boot(data(raffle_done=0)))
              await js("var q=document.querySelector('.q'); q.focus();"
                       "q.value='AB12CD'; q.dispatchEvent(new Event('input',{bubbles:true}));")
              await key("Enter", 13); await asyncio.sleep(0.8)
              # Select-Pfad: „aus der Liste wählen" (admin-Teilnehmer, pcode=null)
              await js("[...document.querySelectorAll('.frow')].find(b=>/Anders/.test(b.innerText))?.click()")
              await asyncio.sleep(0.6)
              info = await js("document.querySelector('.fmarkinfo') ? document.querySelector('.fmarkinfo').innerText : ''")
              check("D1-searchbar-path-shows-real-limit",
                    ("3" in (info or "")) and ("∞" not in (info or "")), info)
              rows_txt = await js("document.querySelector('.flist') ? document.querySelector('.flist').innerText : ''")
              check("D4-wishes-rows-show-type-grade-size",
                    all(x in (rows_txt or "") for x in ("vehicle", "A", "S")), (rows_txt or "")[:160])
              # 4x markieren bei max_wishes=3 → 4. wird geblockt
              await js("for (const b of document.querySelectorAll('.fmark')) b.click();")
              await asyncio.sleep(0.3)
              info2 = await js("document.querySelector('.fmarkinfo') ? document.querySelector('.fmarkinfo').innerText : ''")
              marked_n = await js("document.querySelectorAll('.fmark.is-marked').length")
              check("D1-max-limit-blocks-4th-mark", marked_n == 3 and "3" in (info2 or ""), f"{marked_n} {info2}")
              await shot("r2-01-wishes-limit.png")

              # ============ D3: Select-Pfad ohne pcode → Claim-Feld ============
              await js("document.querySelector('.factions .fsubmit')?.click()")
              await asyncio.sleep(0.5)
              action = await js("window.__STUB_LAST_ACTION__ ? JSON.stringify(window.__STUB_LAST_ACTION__) : null")
              note = await js("document.querySelector('.fnote') ? document.querySelector('.fnote').innerText : ''")
              check("D3-empty-claim-blocks-action-call", action is None, str(action))
              check("D3-empty-claim-shows-param-note", "Code" in (note or "") or "code" in (note or ""), note)
              await shot("r2-02-claim-field.png")
              # Claim-Code eintippen → Confirm → Action MIT pcode
              await js("var f=document.querySelector('.factions .fafield .finput');"
                       "f.value='CLAIM1'; f.dispatchEvent(new Event('input',{bubbles:true}));")
              await js("document.querySelector('.factions .fsubmit')?.click()")
              await asyncio.sleep(0.7)
              action2 = await js("window.__STUB_LAST_ACTION__ ? JSON.stringify(window.__STUB_LAST_ACTION__) : null")
              ok2 = (action2 is not None and "'confirm'" in str(action2).replace('"', "'")
                     and '"pcode":"CLAIM1"' in str(action2).replace(' ', '')
                     and '"loot_id":["L1","L2","L3"]' in str(action2).replace(' ', ''))
              check("D3-confirm-posts-pcode-claim", ok2, str(action2))
              pins_after = await js("localStorage.getItem('connector.flow.pin.sclm.participant')")
              check("D3-claimed-pcode-pinned", pins_after is not None and "CLAIM1" in (pins_after or ""), str(pins_after))
              wait_txt = await js("document.querySelector('.fwait') ? document.querySelector('.fwait').innerText : ''")
              check("D3-confirm-goto-wait", wait_txt is not None and len(wait_txt) > 0, wait_txt)

            if SCENARIO == "d2":
                # ============ D2: Wiedereinstieg raffle_done=1 → direkt results =====
              await navigate(await boot(data(raffle_done=1),
                                        pins={"code": "AB12CD", "sid": "s1",
                                              "participant_id": "p1", "pcode": "CLAIM1"}))
              await js("var q=document.querySelector('.q'); q.focus();"
                       "q.value='AB12CD'; q.dispatchEvent(new Event('input',{bubbles:true}));")
              await key("Enter", 13); await asyncio.sleep(0.8)
              detail = await js("document.querySelector('.fdetail') ? document.querySelector('.fdetail').innerText : ''")
              plist = await js("document.querySelector('.flist') ? document.querySelector('.flist').innerText : ''")
              check("D2-reentry-lands-directly-on-results",
                    "Cutlass Black" in (detail or "") and "Anders" not in (plist or ""),
                    f"detail={(detail or '')[:80]} list={(plist or '')[:80]}")
              btns = await js("document.querySelector('.factions') ? document.querySelector('.factions').innerText : ''")
              check("D2-results-actions-present",
                    ("abgeholt" in (btns or "").lower() or "collected" in (btns or "").lower()), str(btns))
              await shot("r2-03-reentry-results.png")
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
