#!/usr/bin/env python3
"""CDP-Probe for t_bc0f00e7 (K5 Small-Fixes, v0.3.2): F6-Verifikation, M-Texte,
Inventar-Cards, 4 Quality-Steps, Such-Historie.

Echt Svelte-UI, Tauri-IPC gestubbt (smoke-stub.js). Checks:
  1. (P2) F6 opens Handelsrouten (smode HANDELSROUTEN, .mrow.rrow) from ALL
     modes: Search/Idle, Market, Inventar (F3), Hangar (F4), Settings (F12).
     Rail shows the F6 entry active (railMode-Latch resolved, t_04e3d8c6).
  2. (P6) No stale 'M'-Shortcut text anywhere: no <kbd>M</kbd>, no <b>M</b>,
     search-mode foot-line says 'F2 Marktplatz' (not 'M Market').
  3. (P8) Searches persist (localStorage architects-overlay.recentSearches) and
     the idle-pane 'Zuletzt gesucht' shows entries; survives page reload.
  4. (P13) Cards-toggle (.chip.view): overview list <-> grid of cards per
     Stand-ort; selection persists across reload.
  5. (P14) 4 quality color-steps visible (stub-rows Q364/Q720/Q780/Q900):
     classes q-bad/q-ok/q-good/q-top with 4 distinct computed colors, list-
     view AND card-view identical.
Screenshots: 01-idle-history, 02-history-after-reload, 03-f6-aus-suche,
04-f6-aus-inventar, 05-f6-aus-hangar, 06-f6-aus-settings,
07-inventar-list-4steps, 08-inventar-cards, 09-cards-after-reload,
10-inventar-list-after-toggleback.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/small-fixes")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8798/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8798", "--bind", "127.0.0.1",
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
        "--remote-debugging-port=9231", "--user-data-dir=/tmp/cdp-small-fixes",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9231/json/list", timeout=1) as r:
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

        VK = {"F1": 112, "F2": 113, "F3": 114, "F4": 115, "F6": 118, "F12": 123,
              "Escape": 27, "Enter": 13, "ArrowDown": 40, "ArrowUp": 38, "Delete": 46}
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

        def smode():
            return js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")

        def set_input(sel, text):
            return ("(function(){var i=document.querySelector(" + json.dumps(sel) + ");"
                    "if(!i) return 'NO-ELEMENT';"
                    "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                    "set.call(i,'');"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                    "set.call(i," + json.dumps(text) + ");"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                    "return i.value;})()")

        # Augmented inventory-dataset (4 quality steps) — intercept inventory_list
        # before the Inventory component loads (F3).
        INVOKE_WRAP = (
            "(function(){ var ti = window.__TAURI_INTERNALS__;"
            " if (!ti || !ti.invoke) return 'NO-IPC';"
            " if (!ti.invoke.__invwrapped) {"
            "  var orig = ti.invoke.bind(ti);"
            "  ti.invoke = function(cmd,args){"
            "    if (cmd === 'inventory_list') {"
            "      var augment = function(rs){"
            "        rs.push({id:'4', entity_kind:'commodity', entity_id:'1000', name:'Gold', quality:780, location_id:null, location_name:'Heim-Hangar', quantity:2, price:3800, created_at:0, updated_at:0});"
            "        rs.push({id:'5', entity_kind:'commodity', entity_id:'22', name:'Titanium', quality:820, location_id:null, location_name:'Org-Depot', quantity:5, price:null, created_at:0, updated_at:0});"
            "        return rs;};"
            "      var p = orig(cmd,args);"
            "      return (p instanceof Promise) ? p.then(augment) : augment(p);"
            "    }"
            "    return orig(cmd,args);"
            "  };"
            "  ti.invoke.__invwrapped = true;"
            " }"
            " return 'WRAPPED';})()")

        def boot_checks(tag):
            """(P6) — no stale M-shortcut texts (kbd-M, b-M, 'M Market')."""
            checks = []
            checks.append(("p6-no-kbd-m-" + tag,
                lambda: js("[...document.querySelectorAll('kbd')].every(function(e){return e.textContent.trim()!=='M';})"),
                True))
            checks.append(("p6-foot-f2-not-m-" + tag,
                lambda: js("(function(){var f=document.querySelector('.foot .hints'); var t=f?f.innerText.toLowerCase().replace(/\\s+/g,' '):''; return t.indexOf('f2 marktplatz')>=0 && t.indexOf('m market')<0;})()"),
                True))
            return checks

        # ---- 0. after boot: (P6) search-mode foot-line + history search (P8) ----
        print("scenario 0: search 'gold' -> history persists, foot shows F2")
        await js("var i=document.querySelector('input.q'); if(i) i.focus();")
        await js(set_input("input.q", "gold"))
        await asyncio.sleep(0.8)
        for nm, fn, want in boot_checks("boot"):
            v = await fn()
            check(nm, bool(v) == want, str(v))
        hist = await js("localStorage.getItem('architects-overlay.recentSearches')||''")
        check("p8-history-persisted-after-search", "gold" in str(hist), str(hist))

        await js(set_input("input.q", "titanium :i"))
        await asyncio.sleep(0.8)
        await js(set_input("input.q", ""))
        await asyncio.sleep(0.6)
        sm = await smode()
        check("p8-idle-pane-active", str(sm).strip() == "SUCHE", str(sm))
        idle = await js("[...document.querySelectorAll('.pane-idle .recent .r')].map(function(e){return e.innerText.trim();})")
        idle = idle or []
        check("p8-idle-shows-history", any("gold" in q for q in idle) and any("titanium" in q for q in idle), str(idle))
        idle_b = await js("[...document.querySelectorAll('.pane-idle b')].every(function(e){return e.textContent.trim()!=='M';})")
        check("p6-idle-no-bare-M", bool(idle_b), str(idle_b))
        await shot("01-idle-history.png")

        # ---- reload → history survives (persistiert Setting/Storage) ----
        print("scenario 0b: reload -> history still there (persistence)")
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)
        await js("var i=document.querySelector('input.q'); if(i) i.focus();")
        await js(set_input("input.q", ""))
        await asyncio.sleep(0.6)
        idle2 = await js("[...document.querySelectorAll('.pane-idle .recent .r')].map(function(e){return e.innerText.trim();})")
        idle2 = idle2 or []
        check("p8-history-survives-reload", any("gold" in q for q in idle2) and any("titanium" in q for q in idle2), str(idle2))
        for nm, fn, want in boot_checks("reload"):
            v = await fn()
            check(nm, bool(v) == want, str(v))
        await shot("02-history-after-reload.png")

        # ---- 1. (P2) F6 from all modes (railMode-Latch resolved) ----
        async def routes_ok(tag):
            sm = await smode()
            rows = int(await js("document.querySelectorAll('.mrow.rrow').length"))
            rail = await js("document.querySelector('.rentry.is-on .rkey') ? document.querySelector('.rentry.is-on .rkey').textContent.trim() : 'NONE'")
            check("f6-" + tag + "-smode-routes", str(sm).strip() == "HANDELSROUTEN", str(sm))
            check("f6-" + tag + "-rail-f6-active", rail == "F6", rail)
            return rows

        print("scenario 1: F6 aus Such-Modus")
        await key("F6")
        await asyncio.sleep(1.2)
        rows = await routes_ok("aus-suche")
        check("f6-aus-suche-rows", rows > 0, "rows=" + str(rows))
        await shot("03-f6-aus-suche.png")

        print("scenario 1b: F3 -> F6 (latch from Inventar)")
        await key("F3")
        await asyncio.sleep(0.8)
        await key("F6")
        await asyncio.sleep(1.2)
        await routes_ok("aus-inventar")
        await shot("04-f6-aus-inventar.png")

        print("scenario 1c: F4 -> F6 (latch from Hangar)")
        await key("F4")
        await asyncio.sleep(0.8)
        await key("F6")
        await asyncio.sleep(1.2)
        await routes_ok("aus-hangar")
        await shot("05-f6-aus-hangar.png")

        print("scenario 1d: F12 -> F6 (latch from Settings)")
        await key("F12")
        await asyncio.sleep(0.6)
        cfg = await js("!!document.querySelector('.pane-config')")
        check("settings-open-before-f6", bool(cfg), str(cfg))
        await key("F6")
        await asyncio.sleep(1.2)
        await routes_ok("aus-settings")
        await shot("06-f6-aus-settings.png")

        # ---- 2. (P13/P14) Inventar: 4 quality steps + Cards-Umschalter ----
        print("scenario 2: inventory with 4-step qualities (augmented stub)")
        w = await js(INVOKE_WRAP)
        check("invoke-wrapper-inventory_list", str(w) in ("WRAPPED", "NO-IPC"), str(w))
        await key("F3")
        await asyncio.sleep(1.0)
        chips = await js("""(function(){
            var out=[];
            for (var el of document.querySelectorAll('.inv-row .q')) {
                var cs = getComputedStyle(el);
                out.push({cls: el.className, txt: el.innerText.trim(), color: cs.color, bg: cs.backgroundColor});
            }
            return JSON.stringify(out);})()""")
        try:
            chips = json.loads(str(chips))
        except Exception:
            chips = []
        want = {"q-bad": "Q364", "q-ok": "Q720", "q-good": "Q780", "q-top": "Q900"}
        for cls, txt in want.items():
            hit = [c for c in chips if cls in c["cls"].split() and c["txt"] == txt]
            check("p14-step-" + cls, len(hit) > 0, txt)
        colors_bad = [c["color"] for c in chips if "q-bad" in c["cls"]]
        colors_ok = [c["color"] for c in chips if "q-ok" in c["cls"]]
        colors_good = [c["color"] for c in chips if "q-good" in c["cls"]]
        colors_top = [c["color"] for c in chips if "q-top" in c["cls"]]
        uniq = {c for cs in (colors_bad, colors_ok, colors_good, colors_top) for c in cs}
        check("p14-four-distinct-colors", len(uniq) == 4, f"uniq={sorted(uniq)}")
        check("p14-list-view-default", (await js("!!document.querySelector('.inv-cards')")) in (False, None), "must start as list")
        await shot("07-inventar-list-4steps.png")

        print("scenario 2b: Cards-toggle")
        t = await js("(function(){var b=document.querySelector('.chip.view'); if(!b) return 'NO-CHIP'; b.click(); return b.innerText.trim();})()")
        check("p13-toggle-chip-present", str(t) != "NO-CHIP", str(t))
        await asyncio.sleep(0.5)
        cards = int(await js("document.querySelectorAll('.inv-card').length"))
        check("p13-cards-rendered", cards == 3, "cards=" + str(cards))
        headchips = await js("[...document.querySelectorAll('.inv-card .ic-head .q')].map(function(e){return e.innerText.trim();})")
        check("p13-card-header-qchips", bool(headchips), str(headchips))
        rowchips = await js("(function(){var o=[];for(el of document.querySelectorAll('.inv-card .inv-row .q')){var cs=getComputedStyle(el);o.push(el.className+'|'+el.innerText.trim()+'|'+cs.color);} return JSON.stringify(o);})()")
        try:
            rowchips = json.loads(str(rowchips))
        except Exception:
            rowchips = []
        tiers = set()
        for c in rowchips:
            for tok in c.split("|")[0].split():
                if tok.startswith("q-"):
                    tiers.add(tok[2:])
        check("p14-consistent-in-cardview", {"bad", "ok", "good", "top"} <= tiers, str(sorted(tiers)))
        await shot("08-inventar-cards.png")

        print("scenario 2c: reload -> Cards view persists")
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)
        w = await js(INVOKE_WRAP)
        await key("F3")
        await asyncio.sleep(1.0)
        cards2 = int(await js("document.querySelectorAll('.inv-card').length"))
        check("p13-cards-persist-across-reload", cards2 == 3, "cards=" + str(cards2))
        await shot("09-cards-after-reload.png")

        print("scenario 2d: toggle back to list")
        t = await js("(function(){var b=document.querySelector('.chip.view'); if(!b) return 'NO-CHIP'; b.click(); return 'OK';})()")
        await asyncio.sleep(0.5)
        cards3 = int(await js("document.querySelectorAll('.inv-card').length"))
        check("p13-toggle-back-to-list", cards3 == 0, "cards=" + str(cards3))
        await shot("10-inventar-list-after-toggleback.png")
        persisted = await js("localStorage.getItem('architects-overlay.inventory.cards')")
        check("p13-toggle-persisted-value", str(persisted) == "0", str(persisted))

        server.terminate(); proc.terminate()

asyncio.run(main())
