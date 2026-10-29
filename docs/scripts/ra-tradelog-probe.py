#!/usr/bin/env python3
"""CDP-Probe RA (t_1bd5328b) — Trade Log Refresh/Delete/Suggest.

Belegt mit funktionalen DOM-Assertions:
  RA-1 — „Aktualisieren" reicht force=true bis zur IPC-Grenze durch
         (Stub-Capture `get_my_trades` args.force).
  RA-2 — Refresh wirkt SICHTBAR: Stub-Trade-Bestand wird zwischen zwei
         Refreshes geändert (Simuliertes UEX-seitiges Add) → neue Zeile
         erscheint nach Klick.
  RA-3 — Overlay-Delete: „Löschen" → Bestätigungs-Popup → trade_remove
         mit der richtigen ID → Trade verschwindet aus der Liste;
         Reload danach läuft mit force=true.
  RA-4 — Commodity-Suggest ohne gewählten Ort liefert Gold/Janalite/
         Pitambu (leerer Cache-Regel: Frontend-Suggest-Backend ist durch
         cargo tests + reale Cache-DB-Probe belegt; hier die UI-Kante).

Datenquelle: smoke-stub.js (JS-Mirror; Rust-Seite durch cargo tests
belegt: user_trades_force_bypasses_fresh_snapshot, my_trades_force_reports_live_and_hits_provider).
Screenshots nach docs/screens/ra-tradelog/.

Self-contained: startet statischen Server (build/) + headless Chromium.
"""
import asyncio, base64, json, os, pathlib, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens/ra-tradelog"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8813"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9253"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

NOW = int(time.time())

def trade(id_, commodity, cid, terminal, tid, op, scu, price):
    return {"id": id_, "commodity": commodity, "commodity_id": cid,
            "terminal": terminal, "terminal_id": tid, "operation": op,
            "scu": scu, "price": price, "date_added": NOW - id_ * 3600}

# PRICES-Fixture für die Cross-Autocomplete-Semantik (Everus verkauft Gold).
PRICES = json.dumps([
    {"commodity_id": 11, "terminal_id": 21, "buy_price": 1200, "sell_price": 0},
], separators=(",", ":"))

def env_for(trades):
    return (
        "window.__stubState.uex_api_key_set = true;"
        "window.__stubState.uex_secret_key_set = true;"
        "window.__QY_TRADE_PRICES__ = " + PRICES + ";"
        "window.__SHOP_TRADES__ = " + json.dumps(trades, separators=(",", ":")) + ";"
        "window.__SHOP_TRADES_SOURCE__ = 'live';"
        "window.__SHOP_TRADES_NOTE__ = null;"
        "window.__GET_TRADES_CALLS__ = 0;"
        "window.__LAST_GET_TRADES_ARGS__ = null;"
        "window.__LAST_TRADE_REMOVE_ID__ = null;"
        "globalThis.__trkComPool = ["
        "{ id: 11, name: 'Gold' }, { id: 12, name: 'Laranite' }, { id: 13, name: 'Titanium' },"
        "{ id: 31, name: 'Janalite' }, { id: 32, name: 'Pitambu' }];"
    )

BASE_TRADES = [trade(1, "Gold", 11, "Everus Harbor", 21, "buy", 10, 1200)]

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
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
        raise RuntimeError(f"static server on {PORT} did not come up")
    print("static server up on", PORT)

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-ra-tradelog",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{CDP_PORT}/json/list", timeout=1) as r:
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

        async def recv_loop():
            async for raw in cdp:
                m = json.loads(raw)
                if "id" in m and m["id"] in pending:
                    pending[m["id"]].set_result(m)
        asyncio.create_task(recv_loop())

        def ok(resp):
            if "error" in resp:
                raise RuntimeError("CDP error: %s" % resp["error"])
            return resp.get("result", {})

        await call("Page.enable")
        await call("Runtime.enable")

        async def navigate(stub_extra=""):
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
            if stub_extra:
                await call("Page.addScriptToEvaluateOnNewDocument", {"source": stub_extra})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.2)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        VK = {"F2": (113, "F2"), "F3": (114, "F3")}
        async def key(k):
            vk, code = VK[k]
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.2)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = base64.b64decode(r["data"])
            (OUT / name).write_bytes(data)
            print("  shot:", OUT / name, len(data), "bytes")

        async def click_chip(label):
            return await js(
                "(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()")

        async def enter_market():
            await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
            await key("F2")
            await asyncio.sleep(1.2)

        async def enter_routes():
            await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
            await key("F3")
            await asyncio.sleep(1.2)

        def type_input_js_idx(selector, idx, text):
            return ("(function(){"
                    "var els=document.querySelectorAll(" + json.dumps(selector) + ");"
                    "var i=els[" + str(idx) + "];"
                    "if(!i) return 'NOFIELD';"
                    "i.focus();"
                    "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                    "set.call(i," + json.dumps(text) + ");"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:'x'}));"
                    "return i.value;})()")

        async def open_form():
            opened = await js("(function(){var b=[...document.querySelectorAll('.mbar .chip')].find(function(c){return c.textContent.trim()==='+ Trade erfassen';}); if(b){b.click(); return true;} return false;})()")
            await asyncio.sleep(0.4)
            return opened

        async def ta_options():
            return json.loads(str(await js(
                "JSON.stringify([...document.querySelectorAll('.trk-modal .rta-item')].map(function(o){return o.textContent.trim();}))")) or "[]")

        # =================================================================
        # RA-1 + RA-2 — Refresh mit force + sichtbare Wirkung (UEX-Add)
        # =================================================================
        await navigate(env_for(BASE_TRADES))
        await enter_routes()
        tab = await click_chip("Trade Log")
        check("ra-setup-tradelog-tab", tab is True)
        await asyncio.sleep(1.0)

        # Zeilen zählen NUR Trades (das Tages-Aggregat ist ebenfalls .trow —
        # Trade-Zeilen erkennbar am SCU×Preis-Gesamtwert-Titel).
        TRADE_ROWS = ("JSON.stringify([...document.querySelectorAll('.mlist .mrow.trow')]"
                      ".filter(function(r){return r.querySelector('.m-price[title*=\\'Gesamtwert\\']');}).length)")
        rows = json.loads(str(await js(TRADE_ROWS)) or "0")
        check("ra2-initial-row-count-1", rows == 1, "rows=" + str(rows))

        # UEX-seitiger Add wird simuliert: Stub-Bestand erweitern (die
        # Frontend-Liste weiß davon NICHT — nur ein force-Refresh holt sie).
        await js("window.__SHOP_TRADES__ = window.__SHOP_TRADES__.concat([{id:2,commodity:'Titanium',commodity_id:13,terminal:'Baijini Point',terminal_id:23,operation:'sell',scu:4,price:2500,date_added:" + str(NOW) + "}])")
        await js("window.__LAST_GET_TRADES_ARGS__ = null")
        clicked = await click_chip("Aktualisieren")
        check("ra1-refresh-clicked", clicked is True)
        await asyncio.sleep(1.0)
        args = json.loads(str(await js("JSON.stringify(window.__LAST_GET_TRADES_ARGS__)")) or "null")
        check("ra1-force-passed-through", bool(args and args.get("force")), str(args))
        rows2 = json.loads(str(await js(TRADE_ROWS)) or "0")
        check("ra2-new-trade-visible-after-refresh", rows2 == 2, "rows=" + str(rows2))
        await shot("ra2-refresh-shows-new-trade.png")

        # =================================================================
        # RA-3 — Overlay-Delete: Bestätigungs-Popup → trade_remove → Row weg
        # =================================================================
        del_clicked = await js("(function(){var b=[...document.querySelectorAll('.mlist .mrow.trow')].find(function(r){return r.textContent.includes('Baijini');}); if(!b) return 'NOROW'; var s=[...b.querySelectorAll('.btn-ghost')].find(function(x){return x.textContent.trim()==='Löschen';}); if(!s) return 'NOBTN'; s.click(); return true;})()")
        check("ra3-delete-button-found", del_clicked is True, str(del_clicked))
        await asyncio.sleep(0.4)
        confirm_txt = str(await js("(function(){var m=document.querySelector('.confirm-modal'); return m?m.textContent.replace(/\\s+/g,' ').trim().slice(0,140):null;})()"))
        check("ra3-confirm-popup-opens", bool(confirm_txt) and "LÖSCHEN" in confirm_txt.upper() and "GELÖSCHT" in confirm_txt.upper(), str(confirm_txt))
        await js("window.__LAST_TRADE_REMOVE_ID__ = null")
        confirmed = await js("(function(){var b=[...document.querySelectorAll('button.btn')].find(function(x){return (x.textContent.trim()==='Bestätigen'||x.textContent.trim().includes('Bestätig'));}); if(b){b.click(); return true;} return false;})()")
        check("ra3-confirm-clicked", confirmed is True, str(confirmed))
        await asyncio.sleep(1.0)
        removed = str(await js("JSON.stringify(window.__LAST_TRADE_REMOVE_ID__)"))
        check("ra3-trade-remove-invoked-id2", removed == "2", removed)
        rows3 = json.loads(str(await js(TRADE_ROWS)) or "0")
        check("ra3-row-gone-after-delete", rows3 == 1, "rows=" + str(rows3))
        args3 = json.loads(str(await js("JSON.stringify(window.__LAST_GET_TRADES_ARGS__)")) or "null")
        check("ra3-reload-after-delete-force", bool(args3 and args3.get("force")), str(args3))
        await shot("ra3-delete-done.png")

        # =================================================================
        # RA-4 — Suggest ohne gewählten Ort (Gold/Janalite/Pitambu)
        # =================================================================
        opened = await open_form()
        check("ra4-form-opens", opened is True)
        await asyncio.sleep(0.5)
        await js(type_input_js_idx(".trk-modal input.rtext", 0, "Janalite"))
        await asyncio.sleep(0.8)
        opts = await ta_options()
        check("ra4-suggest-janalite-no-terminal", any("Janalite" in o for o in opts), str(opts))
        await shot("ra4-suggest-no-terminal.png")

    proc.terminate()
    server.terminate()

    print()
    print("PASS:", len(PASS), "FAIL:", len(FAIL))
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

if __name__ == "__main__":
    asyncio.run(main())
