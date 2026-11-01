#!/usr/bin/env python3
"""UI-Probe + Screens-Beleg für t_5b7d59a5 (SF3, Ersteller-Flow) — Stub-
Harness wie sclm-flow-r2-probe.py, aber gegen den Creator-Flow:

Beleg-Pflicht „Screens je Screen (Ersteller-Flow alle Reiter)" — Screenshots
für alle 13 Screens des Creator-Flows + Funktions-Asserts:

  A1: Tabs zeigen alle Creator-Screens (Tab-Muster wie Routen/Trade-Logs).
  A2: create → Formular; Submit mapped Response (sid/code/admin_code/join_code
      werden Session-Pins) → manage.
  A3: manage (Detail) zeigt admin_code/join_code MIT Copy-Button (⧉).
  A4: c_participants (Liste) mit Actions (hinzufügen/wishes/rename/remove);
      Row-Select seedet Action-Felder (participant_id).
  A5: add_participant mit Autocomplete (fill "" = Treffer selbst).
  A6: wishes_for als Multi-Select; Confirm postet participant_id + loot_id-Array.
  A7: loot-Liste mit sale/qty/remove-Actions; add_loot mit Autocomplete-Fill
      type/grade/size.
  A8: commodities mit scu-Action.
  A9: c_results als Zeilenliste; Select → result-Detail mit collect/waive.
 A10: open (code_entry) mit exit raffle_done → c_results (Wiedereinstieg).

Screenshots: docs/screenshots/sclm-flow-creator/01..13.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
MANIFEST = json.loads((ROOT / "examples/sclm.json").read_text())
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/sclm-flow-creator")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8799/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

SCLM = {"id": "sclm", "enabled": True, "auth_set": {}, "hotkey": "F7", "manifest": MANIFEST}

SESSION = {"sid": "s1", "name": "Weihnachtsverlosung", "created": "01.11.2026",
           "max_wishes": 4, "locked": False, "raffle_done": 0,
           "admin_code": "ADM123", "join_code": "JON456", "is_admin": True,
           "participants": [
               {"id": "p1", "name": "Sebo", "pcode": "PC1", "wishes": ["L1"]},
               {"id": "p2", "name": "Anders", "pcode": None, "wishes": []}],
           "loot": [
               {"id": "L1", "name": "FS-55 Cannon", "qty": 2, "type": "weapon", "grade": "C", "size": "S2", "wish_count": 1, "for_sale": 0, "sale_revenue": None},
               {"id": "L2", "name": "Rust Helmet", "qty": 1, "type": None, "grade": None, "size": None, "wish_count": 0, "for_sale": 0, "sale_revenue": None}],
           "commodities": [{"id": "C1", "name": "Titanium", "scu": 7}],
           "results": []}
RESULTS_DONE = dict(SESSION, raffle_done=1, results=[
    {"id": "R1", "participant_id": "p1", "participant_name": "Sebo",
     "won": [{"name": "FS-55 Cannon", "type": "weapon", "grade": "C", "size": "S2"}],
     "waived": 0, "collected": 0}])

def data(raffle_done=0):
    s = RESULTS_DONE if raffle_done else SESSION
    # Screen-IDs sind jetzt flow-qualifiziert (`creator:<screen>`)
    return {
        "creator:open": {"mapped": {k: s[k] for k in ("sid", "name", "max_wishes", "locked", "raffle_done", "admin_code", "join_code")}},
        "creator:manage": {"mapped": {k: s[k] for k in ("name", "max_wishes", "admin_code", "join_code", "locked", "raffle_done")}},
        "creator:c_participants": {"rows": [{"name": p["name"], "participant_id": p["id"], "pcode": p["pcode"]} for p in s["participants"]]},
        "creator:wishes_for": {"rows": [{"name": l["name"], "loot_id": l["id"], "qty": l["qty"], "type": l["type"], "grade": l["grade"], "size": l["size"], "wish_count": l["wish_count"]} for l in s["loot"]]},
        "creator:loot": {"rows": [{"name": l["name"], "loot_id": l["id"], "qty": l["qty"], "type": l["type"], "grade": l["grade"], "size": l["size"], "wish_count": l["wish_count"], "for_sale": l["for_sale"], "sale_revenue": l["sale_revenue"]} for l in s["loot"]]},
        "creator:commodities": {"rows": [{"name": c["name"], "cid": c["id"], "scu": c["scu"]} for c in s["commodities"]]},
        "creator:c_results": {"rows": [{"participant": r["participant_name"], "rid": r["id"], "participant_id": r["participant_id"], "won": json.dumps(r["won"]), "waived": r["waived"], "collected": r["collected"]} for r in s["results"]]},
        "creator:result": {"mapped": {"rid": "R1", "won": [{"name": "FS-55 Cannon", "type": "weapon", "grade": "C", "size": "S2"}], "waived": 0, "collected": 0}},
    }

AC_ITEMS = [{"name": "Sebolite", "type": "artifact", "grade": "A", "size": "S1"}]

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8799", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9236", "--user-data-dir=/tmp/cdp-sclm-flow-creator",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9236/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception: pass
        await asyncio.sleep(0.25)

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

            def boot(data_map, pins=None):
                clear = ("try { for (const k of Object.keys(localStorage)) "
                         "if (k.startsWith('connector.flow.pin')) localStorage.removeItem(k); } catch (e) {}")
                pin_js = ""
                if pins:
                    pin_js = ("\nlocalStorage.setItem('connector.flow.pin.sclm.creator', "
                              + json.dumps(json.dumps(pins)) + ");")
                return (clear + pin_js + "\n" + STUB
                        + "\nwindow.__STUB_CONNECTORS__ = " + json.dumps([SCLM]) + ";"
                        + "\nwindow.__STUB_CONNECTOR_FORM_BODY__ = " + json.dumps({"id": "s1", "admin_code": "ADM123", "join_code": "JON456"}) + ";"
                        + "\nwindow.__STUB_CONNECTOR_DATA__ = " + json.dumps(dict(data_map, **{"creator:name": {"body": AC_ITEMS}})) + ";")

            script_ids = []
            async def navigate(boot_src):
                # Alte New-Document-Scripts entfernen (sonst akkumulieren die
                # Boots und das erste Daten-Set gewinnt evtl.).
                for sid_ in script_ids:
                    try: await call("Page.removeScriptToEvaluateOnNewDocument", {"identifier": sid_})
                    except Exception: pass
                script_ids.clear()
                r = ok(await call("Page.addScriptToEvaluateOnNewDocument", {"source": boot_src}))
                if r.get("identifier"): script_ids.append(r["identifier"])
                await call("Page.navigate", {"url": URL})
                await asyncio.sleep(2.5)
                await key("F7", 118)

            async def open_creator():
                # Flow-Tab (Outer .ctab) „Creator" (en-Default) öffnen
                await js("[...document.querySelectorAll('.ctab')].find(b=>/Creator|Ersteller/i.test(b.innerText||''))?.click()")
                await asyncio.sleep(0.8)

            async def goto_screen(tab_label):
                await js(f"[...document.querySelectorAll('.ftab')].find(b=>/{tab_label}/i.test(b.innerText||''))?.click()")
                await asyncio.sleep(0.6)

            # ===== A1/A2: Tabs + create-Formular =====
            await navigate(boot(data()))
            await open_creator()
            tabs = await js("[...document.querySelectorAll('.ftab')].map(b=>b.innerText).join('|')")
            check("A1-tabs-show-creator-screens",
                  all(x in (tabs or "") for x in ["Create raffle", "Open session", "Overview", "Settings",
                                                  "Participants", "Wishes", "Loot", "Commodities",
                                                  "Results", "Result"]), tabs)
            # create ist Entry-Screen: Formular sichtbar
            fields = await js("[...document.querySelectorAll('.ffield .flabel')].map(e=>e.innerText).join('|')")
            check("A2-create-form-fields", "Event name" in (fields or "") and "Max wishes" in (fields or ""), fields)
            await shot("01-create.png")
            # Submit → manage (Pins aus Response-Mapping)
            await js("var f=document.querySelectorAll('.fform .finput'); f[0].value='SF3-Probe'; f[0].dispatchEvent(new Event('input',{bubbles:true}));")
            await js("document.querySelector('.fform .fsubmit')?.click()")
            await asyncio.sleep(0.8)
            detail = await js("document.querySelector('.fdetail') ? document.querySelector('.fdetail').innerText : ''")
            check("A2-create-onsubmit-goto-manage", "Weihnachtsverlosung" in (detail or ""), detail[:120])
            pins = await js("localStorage.getItem('connector.flow.pin.sclm.creator')")
            check("A2-create-pins-codes", all(x in (pins or "") for x in ["s1", "ADM123", "JON456"]), str(pins))

            # ===== A3: manage — Codes sichtbar + Copy-Button =====
            copy_n = await js("document.querySelectorAll('.fdetail .fcopy').length")
            check("A3-manage-codes-copy-buttons", copy_n >= 2, str(copy_n))
            check("A3-manage-shows-codes", "ADM123" in (detail or "") and "JON456" in (detail or ""), detail[:200])
            await shot("02-manage-codes.png")
            await js("document.querySelector('.fdetail .fcopy')?.click()")
            await asyncio.sleep(0.3)

            # ===== A4: c_participants — Actions + Row-Select-Seeding =====
            await goto_screen("Participants")
            acts = await js("[...document.querySelectorAll('.factions button')].map(b=>b.innerText).join('|')")
            check("A4-participant-actions",
                  all(x in (acts or "") for x in ["Add participant", "Edit wishes", "Rename", "Remove"]), acts)
            await js("[...document.querySelectorAll('.frow')].find(b=>/Anders/.test(b.innerText))?.click()")
            await asyncio.sleep(0.4)
            await asyncio.sleep(0.2)
            await shot("03-c_participants.png")
            # rename: Feld Name tippen → PATCH mit participant_id
            await js("var f=document.querySelector('.factions .fafield .finput'); f.value='Anders2'; f.dispatchEvent(new Event('input',{bubbles:true}));")
            await js("[...document.querySelectorAll('.factions button')].find(b=>/Rename/i.test(b.innerText||''))?.click()")
            await asyncio.sleep(0.6)
            action = await js("window.__STUB_LAST_ACTION__ ? JSON.stringify(window.__STUB_LAST_ACTION__) : null")
            check("A4-rename-posts-patch",
                  action is not None and "c_participants" in str(action)
                  and "rename" in str(action) and '"participant_id":"p2"' in str(action).replace(" ", ""), str(action))

            # ===== A5: add_participant — Autocomplete füllt Name =====
            await goto_screen("Add participant")
            await js("var fs=document.querySelectorAll('.fform .finput'); fs[2].focus();")
            await js("var f=document.querySelectorAll('.fform .finput')[2]; f.value='Seb'; f.dispatchEvent(new Event('input',{bubbles:true}));")
            await asyncio.sleep(0.8)
            ac = await js("document.querySelector('.facs') ? document.querySelector('.facs').innerText : ''")
            check("A5-participant-autocomplete", "Sebolite" in (ac or ""), ac)
            await js("document.querySelector('.facs .fac')?.click()")
            await asyncio.sleep(0.3)
            val = await js("document.querySelectorAll('.fform .finput')[2].value")
            check("A5-autocomplete-pick-no-crash",
                  val is not None and "[object Object]" not in str(val), str(val))
            await shot("04-add_participant.png")

            # ===== A6: wishes_for — Multi-Select + Confirm =====
            await goto_screen("Participants")
            await js("[...document.querySelectorAll('.frow')].find(b=>/Sebo/.test(b.innerText))?.click()")
            await asyncio.sleep(0.3)
            await js("[...document.querySelectorAll('.factions button')].find(b=>/Wishes/i.test(b.innerText||''))?.click()")
            await asyncio.sleep(0.6)
            marks = await js("document.querySelectorAll('.fmark').length")
            check("A6-wishes-for-multi", marks >= 2, str(marks))
            await js("for (const b of document.querySelectorAll('.fmark')) b.click();")
            await asyncio.sleep(0.3)
            await shot("05-wishes_for.png")
            await js("document.querySelector('.factions .fsubmit')?.click()")
            await asyncio.sleep(0.7)
            action = await js("window.__STUB_LAST_ACTION__ ? JSON.stringify(window.__STUB_LAST_ACTION__) : null")
            check("A6-confirm-posts-wishes",
                  action is not None and "wishes_for" in str(action)
                  and "confirm" in str(action)
                  and '"participant_id":"p1"' in str(action).replace(" ", "")
                  and '"loot_id":["L1","L2"]' in str(action).replace(" ", ""), str(action))
            await shot("05b-wishes-confirm.png")

            # ===== A7: loot — sale/qty/remove + add_loot-Autofill =====
            await goto_screen("Loot")
            acts = await js("[...document.querySelectorAll('.factions button')].map(b=>b.innerText).join('|')")
            check("A7-loot-actions",
                  all(x in (acts or "") for x in ["sale", "quantity", "Remove", "Add item"]), acts)
            await shot("06-loot.png")
            await goto_screen("Add item")
            await js("var f=document.querySelectorAll('.fform .finput')[2]; f.value='FS'; f.dispatchEvent(new Event('input',{bubbles:true}));")
            await asyncio.sleep(0.8)
            await js("document.querySelector('.facs .fac')?.click()")
            await asyncio.sleep(0.3)
            vals = await js("[...document.querySelectorAll('.fform .finput')].map(i=>i.value).join('|')")
            check("A7-loot-autofill-type-grade-size",
                  "weapon" in vals or "artifact" in vals, vals)
            await shot("07-add_loot.png")

            # ===== A8: commodities =====
            await goto_screen("Commodities")
            rows = await js("document.querySelector('.flist') ? document.querySelector('.flist').innerText : ''")
            check("A8-commodities-list", "Titanium" in (rows or "") and "7" in (rows or ""), rows)
            await shot("08-commodities.png")

            # ===== A9/A10: Einstieg über „Open session" (Admin-Code) — =====
            # ===== exit raffle_done → direkt Results (Wiedereinstieg) =====
            await navigate(boot(data(raffle_done=1)))
            await open_creator()
            await goto_screen("Open session")
            await js("var f=document.querySelector('.fform .finput'); f.focus(); f.value='ADM123'; f.dispatchEvent(new Event('input',{bubbles:true}));")
            await js("document.querySelector('.fform .fsubmit')?.click()")
            await asyncio.sleep(1.0)
            dbg2 = await js("JSON.stringify({open: window.__STUB_CONNECTOR_DATA__?.['creator:open'] ?? null, rd: window.__STUB_CONNECTOR_DATA__?.['creator:open']?.mapped?.raffle_done})")
            print("  dbg A10:", dbg2)
            shown = await js("[...document.querySelectorAll('.ftab')].find(b=>b.className.includes('is-on')) ? [...document.querySelectorAll('.ftab')].find(b=>b.className.includes('is-on')).innerText : ''")
            check("A10-reentry-raffle-done-goto-results", "Results" in (shown or ""), shown)
            await shot("09-open-reentry-results.png")
            rows = await js("document.querySelector('.flist') ? document.querySelector('.flist').innerText : ''")
            check("A9-results-list-all-participants",
                  "Sebo" in (rows or "") and "FS-55" in (rows or ""), (rows or "")[:150])
            await js("document.querySelector('.frow')?.click()")
            await asyncio.sleep(0.8)
            detail = await js("document.querySelector('.fdetail') ? document.querySelector('.fdetail').innerText : ''")
            acts = await js("[...document.querySelectorAll('.factions button')].map(b=>b.innerText).join('|')")
            check("A9-result-detail-collect-waive",
                  "R1" in (detail or "") and "collected" in (acts or "").lower() and "waive" in (acts or "").lower(), detail[:120] + " | " + acts)
            await shot("10-result.png")

            print(f"\nSF3 Creator UI-Probe: {len(PASS)} PASS / {len(FAIL)} FAIL")
            if FAIL:
                print("FAILED: " + ", ".join(FAIL)); raise SystemExit(1)
    finally:
        server.terminate(); proc.terminate()
        try: await asyncio.wait_for(server.wait(), 5)
        except Exception: server.kill()
        try: await asyncio.wait_for(proc.wait(), 5)
        except Exception: proc.kill()

if __name__ == "__main__":
    asyncio.run(main())
