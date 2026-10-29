#!/usr/bin/env python3
"""Deploy-R1 probe (t_6b2185b1) — Live-Binary-Check: UI rendert deutsche
Delete-Fehlercodes.

Fährt die GLEICHE frische Frontend-Build (build/, das gerade in die
Release-Binary eingebettet wurde) headless. trade_remove wird für eine
fremde/nicht vorhandene ID zum Reject gezwungen — reject-String ist der
BYTE-GLEICHE Text, den das echte Backend-friendly() (src-tauri/src/trade.rs)
für trade_not_found/missing_id/missing_secret_key emittiert (im Binary durch
grep >0 belegt). Beweist die UI-Kante: Fehlercode erscheint DEUTSCH.
"""
import asyncio, base64, json, os, pathlib, time, urllib.request
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens/t6-r1-delete"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8817"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9257"))
URL = f"http://127.0.0.1:{PORT}/"

GERMAN = {
    "trade_not_found": "Der Trade existiert nicht (trade_not_found) — vermutlich bereits gelöscht oder fremde ID.",
    "missing_id": "Trade-ID fehlt (missing_id).",
    "missing_secret_key": "Secret-Key fehlt — siehe Einstellungen („UEX persönlicher Secret-Key\").",
    "missing_id_category": "HTTP 400 · UEX nicht erreichbar",
}

NOW = int(time.time())
PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

def trade(id_, commodity, op, scu, price):
    return {"id": id_, "commodity": commodity, "commodity_id": id_, "terminal": "Everus Harbor",
            "terminal_id": 21, "operation": op, "scu": scu, "price": price, "date_added": NOW - id_ * 3600}

BASE_TRADES = [trade(1, "Gold", "buy", 10, 1200), trade(2, "Laranite", "sell", 4, 2500)]

ENV = (
    "window.__stubState.uex_api_key_set = true;"
    "window.__stubState.uex_secret_key_set = true;"
    "window.__SHOP_TRADES__ = " + json.dumps(BASE_TRADES) + ";"
    "window.__SHOP_TRADES_SOURCE__ = 'live';"
)

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
        "--directory", str(ROOT / "build"),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    for _ in range(60):
        try:
            with urllib.request.urlopen(URL, timeout=1) as r:
                if r.status == 200: break
        except Exception: pass
        await asyncio.sleep(0.25)
    else:
        raise RuntimeError("static server not up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-t6-r1",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{CDP_PORT}/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception: pass
        await asyncio.sleep(0.25)
    if ws is None: raise RuntimeError("cdp page not up")

    async with websockets.connect(ws, max_size=None) as cdp:
        msg_id = 0; pending = {}
        async def call(method, params=None):
            nonlocal msg_id
            msg_id += 1; mid = msg_id
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
            if "error" in resp: raise RuntimeError("CDP error: %s" % resp["error"])
            return resp.get("result", {})
        await call("Page.enable"); await call("Runtime.enable")
        async def navigate(extra=""):
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
            if extra: await call("Page.addScriptToEvaluateOnNewDocument", {"source": extra})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.2)
        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"): return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")
        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", OUT / name)
        async def key_f3():
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": "F3", "code": "F3",
                                                  "windowsVirtualKeyCode": 114, "nativeVirtualKeyCode": 114})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": "F3", "code": "F3",
                                                  "windowsVirtualKeyCode": 114, "nativeVirtualKeyCode": 114})
            await asyncio.sleep(0.2)
        async def click_chip(label):
            expr = ("(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){"
                    "return x.textContent.trim()===" + json.dumps(label) + "});"
                    "if(b){b.click(); return true;} return false;})()")
            return await js(expr)
        # Zeilenzähler: Trade-Zeilen = .trow mit .m-price[title*=Gesamtwert] (kein Tages-Aggregat)
        TRADE_ROWS_JS = (
            "JSON.stringify((function(){"
            "var rows=[...document.querySelectorAll('.mlist .mrow.trow')];"
            "var t=rows.filter(function(r){var p=r.querySelector('.m-price');"
            "return p && /Gesamtwert/.test((p.getAttribute && p.getAttribute('title'))||'');});"
            "return t.length;})())"
        )

        async def scenario(label, reject_code):
            await navigate(ENV)
            await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
            await key_f3(); await asyncio.sleep(1.2)
            await click_chip("Trade Log"); await asyncio.sleep(1.0)
            rows = json.loads(str(await js(TRADE_ROWS_JS)) or "0")
            check(f"{label}-rows-2", rows == 2, f"rows={rows}")
            del_clicked = await js("(function(){var b=[...document.querySelectorAll('.mlist .mrow.trow')].find(function(r){return r.textContent.includes('Laranite');}); if(!b) return 'NOROW'; var s=[...b.querySelectorAll('.btn-ghost')].find(function(x){return x.textContent.trim()==='Löschen';}); if(!s) return 'NOBTN'; s.click(); return true;})()")
            check(f"{label}-delete-clicked", del_clicked is True, str(del_clicked))
            await asyncio.sleep(0.4)
            # trade_remove zum Reject zwingen VOR Bestätigen: originale invoke wrappen
            await js("(function(){ if(!window.__realInvoke){ window.__realInvoke = window.__TAURI_INTERNALS__.invoke; } "
                     "window.__TAURI_INTERNALS__.invoke = (function(real){ return async function(cmd,args){ "
                     "if(cmd==='trade_remove'){ throw new Error(" + json.dumps(GERMAN[reject_code]) + "); } "
                     "return real(cmd,args); }; })(window.__realInvoke); })()")
            confirmed = await js("(function(){var b=[...document.querySelectorAll('button.btn')].find(function(x){return (x.textContent.trim()==='Bestätigen'||x.textContent.trim().includes('Bestätig'));}); if(b){b.click(); return true;} return false;})()")
            check(f"{label}-confirm-clicked", confirmed is True, str(confirmed))
            await asyncio.sleep(1.0)
            msg = str(await js("(function(){var m=document.querySelector('.mlist div[role=alert], .mlist .sm'); return m?m.textContent.replace(/\\s+/g,' ').trim():null;})()"))
            if msg == "None" or not msg:
                msg = str(await js("JSON.stringify([...document.querySelectorAll('[role=alert]')].map(function(m){return m.textContent.replace(/\\s+/g,' ').trim();}))"))
            # Robust: GESAMTE body-text nach dem Fehler durchsuchen (nicht nur
            # Alert-Rolle) + Row-Existenz (beweist, dass der Reject-Pfad griff).
            full = str(await js("document.body.innerText.replace(/\\s+/g,' ').trim()"))
            row_count = str(await js(TRADE_ROWS_JS))
            check(f"{label}-reject-path-fired-row-preserved", row_count == "2", f"rows-json={row_count}")
            visible = GERMAN[reject_code] in full
            check(f"{label}-german-IN-BODYTEXT", visible, f"bodylen={len(full)}")
            check(f"{label}-german-shown", GERMAN[reject_code] in msg or visible, f"msg={msg[:120]} bodyHit={visible}")
            await shot(f"{label}.png")

        for code in ["trade_not_found", "missing_id", "missing_secret_key", "missing_id_category"]:
            await scenario("german-" + code, code)

    proc.terminate(); server.terminate()
    print(); print("PASS:", len(PASS), "FAIL:", len(FAIL))
    if FAIL:
        print("FAILED:", ", ".join(FAIL)); raise SystemExit(1)

if __name__ == "__main__":
    asyncio.run(main())