#!/usr/bin/env python3
"""CDP-Probe für P3 (Market-Auto-Sync bei stale Daten) auf dem Integrat t_0fb56efd.

Belegt:
  1. Fresh-Einstieg → normaler Load (Spinner erlaubt), fetched_at wird
     PERSISTIERT in localStorage (Auflage C: mFetchedAt ist nicht mehr rein
     in-memory).
  2. Nach Verlassen des Markts (P5: Liste leer) + künstlich veraltete
     fetched_at (> MARKET_STALE_SECS) → Wiedereinstieg löst einen NEUEN
     get_marketplace aus (bg-Pfad), ohne dass der Spinner erscheint.
  3. Der Hintergrund-Fetch liefert: Liste wieder gefüllt, fetched_at aktualisiert.

Datenquelle: smoke-stub.js (JS-Mirror des Provider-Kontrakts); der Stub-Aufruf
wird per invoke-Wrapper um 700 ms verzögert, damit der bg-Fetch beobachtbar ist.
Self-contained: startet den statischen Server (build/) und headless Chromium.
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
URL = os.environ.get("PROBE_URL", "http://127.0.0.1:8796/")

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8796", "--bind", "127.0.0.1",
        "--directory", str(ROOT / "build"),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    import urllib.request
    for _ in range(60):
        try:
            with urllib.request.urlopen(URL, timeout=1) as r:
                if r.status == 200:
                    break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    else:
        raise RuntimeError("static server on 8796 did not come up")
    print("static server up on 8796")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9228", "--user-data-dir=/tmp/cdp-p3-stale",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9228/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        raise RuntimeError("chromium CDP page target did not come up")

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

        console_events = []
        async def recv_loop():
            async for raw in cdp:
                msg = json.loads(raw)
                if "id" in msg and msg["id"] in pending:
                    pending[msg["id"]].set_result(msg)
                elif msg.get("method") == "Runtime.exceptionThrown":
                    d = msg["params"]["exceptionDetails"]
                    console_events.append("EXC: " + (d.get("text") or "") + " " + (d.get("exception", {}).get("description", "") or "")[:300])
                elif msg.get("method") == "Log.entryAdded":
                    e = msg["params"]["entry"]
                    console_events.append("LOG: " + e.get("text", "")[:200])
        asyncio.create_task(recv_loop())

        def ok(resp):
            if "error" in resp:
                raise RuntimeError("CDP error: %s" % resp["error"])
            return resp.get("result", {})

        await call("Page.enable")
        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride", {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        # invoke-Log + get_marketplace um 700 ms verzögern (bg-Fetch beobachtbar)
        await call("Runtime.evaluate", {"expression": """
            (function(){
              var ti = window.__TAURI_INTERNALS__;
              if (ti && ti.invoke && !ti.invoke.__logged){
                var orig = ti.invoke.bind(ti);
                ti.invoke = function(cmd,args){
                  window.__calls = window.__calls || [];
                  window.__calls.push(cmd);
                  if (cmd === 'get_marketplace') {
                    return new Promise(function(res,rej){
                      setTimeout(function(){ orig(cmd,args).then(res,rej); }, 700);
                    });
                  }
                  return orig(cmd,args);
                };
                ti.invoke.__logged = true;
              }
            })();
        """, "returnByValue": True})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(3.0)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            v = r.get("result", {}).get("value")
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return v

        async def key(code, keyname, vk=0, mods=None):
            await call("Input.dispatchKeyEvent", {"type": "keyDown", "code": code, "key": keyname, "windowsVirtualKeyCode": vk, "modifiers": mods or 0})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "code": code, "key": keyname, "windowsVirtualKeyCode": vk, "modifiers": mods or 0})

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = r["data"]
            out = OUT / f"{name}.png"
            out.write_bytes(__import__("base64").b64decode(data))
            print(f"  shot: {out} {out.stat().st_size} bytes")

        async def market_rows():
            return await js("[...document.querySelectorAll('.pane-market .mrow')].length")

        async def spinner_visible():
            return await js("!!document.querySelector('.spin')")

        # Re-Instrumentierung nach Boot (Stub definiert __TAURI_INTERNALS__ evtl. lazy)
        await js("(function(){ var ti = window.__TAURI_INTERNALS__; if (ti && ti.invoke && !ti.invoke.__logged){ var orig = ti.invoke.bind(ti); ti.invoke = function(cmd,args){ window.__calls = window.__calls || []; window.__calls.push(cmd); if (cmd === 'get_marketplace') { return new Promise(function(res,rej){ setTimeout(function(){ orig(cmd,args).then(res,rej); }, 700); }); } return orig(cmd,args); }; ti.invoke.__logged = true; } })()")

        # reset persisted fetched_at for a clean run
        await js("localStorage.removeItem('architects-overlay.market.fetchedAt')")

        # ---- 1) fresh entry: normal load, persisted fetched_at ----
        print("scenario 1: fresh market entry (normal load)")
        calls_before = await js("(window.__calls||[]).filter(function(c){return c==='get_marketplace'}).length")
        await key("F2", "F2", vk=110)  # F2/Rail = Realit (t_a39f6811: M-Shortcut removed)
        await asyncio.sleep(1.5)
        rows1 = await market_rows()
        check("p3-fresh: market list filled", (rows1 or 0) > 0, f"rows={rows1}")
        check("p3-fresh: normal load fetched (1 get_marketplace)",
              (await js("(window.__calls||[]).filter(function(c){return c==='get_marketplace'}).length")) - calls_before == 1)
        stored = await js("Number(localStorage.getItem('architects-overlay.market.fetchedAt')||0)")
        nowsec = await js("Math.floor(Date.now()/1000)")
        check("c-fix: fetched_at persisted in localStorage", stored > 0 and nowsec - stored < 120, f"stored={stored} now={nowsec}")
        await shot("p3-01-fresh-entry")

        # ---- 2) leave market (P5) then re-enter with stale fetched_at ----
        print("scenario 2: leave → stale re-entry (bg sync, no spinner)")
        await key("Tab", "Tab", vk=9)
        await asyncio.sleep(0.6)
        phase_txt = await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")
        rows_after_leave = await market_rows()
        check("p5-precondition: list empty after leaving market", (rows_after_leave or 0) == 0, f"rows={rows_after_leave}")
        check("p5-precondition: back in search pane", (phase_txt or "").strip() == "SUCHE", phase_txt)

        await js("localStorage.setItem('architects-overlay.market.fetchedAt', String(Math.floor(Date.now()/1000) - 400))")
        # Reload: In-Memory-Weg ist nach P5 leer — der persistierte Wert ist
        # die einzige Frische-Quelle; nach Reload ist er >5 min alt (stale).
        await call("Page.reload", {})
        await asyncio.sleep(3.0)
        await js("(function(){ var ti = window.__TAURI_INTERNALS__; if (ti && ti.invoke && !ti.invoke.__logged){ var orig = ti.invoke.bind(ti); ti.invoke = function(cmd,args){ window.__calls = window.__calls || []; window.__calls.push(cmd); if (cmd === 'get_marketplace') { return new Promise(function(res,rej){ setTimeout(function(){ orig(cmd,args).then(res,rej); }, 700); }); } return orig(cmd,args); }; ti.invoke.__logged = true; } })()")
        stored_reload = await js("Number(localStorage.getItem('architects-overlay.market.fetchedAt')||0)")
        check("c-fix: fetched_at survived reload (stale, >5 min)", 0 < stored_reload < (await js("Math.floor(Date.now()/1000)")) - 300, f"stored={stored_reload}")
        calls_stale = await js("(window.__calls||[]).filter(function(c){return c==='get_marketplace'}).length")
        await key("m", "m", vk=77, mods=2)
        # mid-flight (stub verzögert 700 ms): Spinner darf NICHT erscheinen
        await asyncio.sleep(0.35)
        spin = await spinner_visible()
        check("p3-stale: NO spinner during stale re-entry (bg path)", not spin, f"spin={spin}")
        await shot("p3-02-stale-reentry-nospinner")
        await asyncio.sleep(1.5)
        calls_after = await js("(window.__calls||[]).filter(function(c){return c==='get_marketplace'}).length")
        check("p3-stale: stale entry triggered new get_marketplace", calls_after - calls_stale == 1, f"delta={calls_after-calls_stale}")
        rows2 = await market_rows()
        check("p3-stale: list refilled after bg sync", (rows2 or 0) > 0, f"rows={rows2}")
        stored2 = await js("Number(localStorage.getItem('architects-overlay.market.fetchedAt')||0)")
        check("p3-stale: fetched_at refreshed after bg sync", stored2 > stored, f"{stored} -> {stored2}")
        await shot("p3-03-stale-synced")

        check("no-console-exceptions", not console_events, "; ".join(console_events[:3]))

    proc.terminate()
    server.terminate()
    print("\n===== RESULTAT =====")
    print(f"PASS: {len(PASS)}  FAIL: {len(FAIL)}")
    for f in FAIL:
        print("FAILED:", f)
    raise SystemExit(1 if FAIL else 0)

asyncio.run(main())
