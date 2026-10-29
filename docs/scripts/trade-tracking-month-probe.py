#!/usr/bin/env python3
"""CDP-Probe PT-Monatswechsel (t_7fdc1317, Review-Finding 1).

Reproduziert den Reviewer-Bug: Tages-Aggregat über Monatsgrenzen.
Fixtures: 01.11.2026 / 31.10. / 30.10. / 29.10.2026.
Erwartet (neuester Tag ZUERST): 1.11. | 31.10. | 30.10. | 29.10.
"""
import asyncio, base64, json, os, pathlib, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens/trade-tracking"
PORT = int(os.environ.get("PROBE_PORT", "8805"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9241"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

NOW = int(time.time())

def trade(id_, op, commodity, cid, terminal, tid, scu, price, date_added):
    return {
        "id": id_, "terminal_id": tid, "commodity_id": cid,
        "operation": op, "scu": scu, "price": price, "date_added": date_added,
        "commodity": commodity, "terminal": terminal,
        "vehicle": None, "fleet_name": None, "organization": None,
    }

# NOW = 29.10.2026 (Probe-Laufzeit) → Monatswechsel-Fixtures
TRADES = json.dumps([
    trade(1, "buy",  "Gold",     11, "Everus Harbor", 21, 10, 1000, NOW + 3 * 86400),  # 01.11.
    trade(2, "sell", "Laranite", 12, "Port Olisario", 22, 5, 2200, NOW + 2 * 86400),  # 31.10.
    trade(3, "buy",  "Titanium", 13, "Baijini Point", 23, 4, 500,  NOW + 1 * 86400),  # 30.10.
    trade(4, "buy",  "Gold",     11, "Everus Harbor", 21, 2, 900,  NOW - 3600),       # 29.10.
], separators=(",", ":"))

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
    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-trade-month",
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

    async with websockets.connect(ws, max_size=None) as cdp:
        msg_id = 0
        pending = {}

        async def call(method, params=None):
            nonlocal msg_id
            msg_id += 1
            fut = asyncio.get_event_loop().create_future()
            pending[msg_id] = fut
            await cdp.send(json.dumps({"id": msg_id, "method": method, "params": params or {}}))
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

        env = (
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "window.__SHOP_TRADES__ = " + TRADES + ";"
            "window.__SHOP_TRADES_SOURCE__ = 'cache';"
            "window.__SHOP_TRADES_NOTE__ = null;"
        )
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": env})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.2)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        async def key(k, vk, code):
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.2)

        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        await key("F3", 114, "F3")
        await asyncio.sleep(1.2)
        await js("(function(){var b=[...document.querySelectorAll('.mbar2 .chip')].find(function(c){return c.textContent.trim()==='Tracking';}); if(b) b.click();})()")
        await asyncio.sleep(1.2)

        rows = json.loads(str(await js(
            "JSON.stringify([...document.querySelectorAll('.mlist .mrow.trow')]"
            ".map(function(r){return r.textContent.replace(/\\s+/g,' ').trim();}))")) or "[]")
        # Tages-Zeilen: tragen 'Trade'/'Trades' als Zähler
        day_rows = [r for r in rows if "Trade" in r.split("|")[0] or " Trades " in r]
        day_rows = [r for r in rows if r[:6].count(".") >= 2 and r[0].isdigit()]
        day_text = [r.split(" ")[0] for r in day_rows]
        print("  day-rows:", day_text)
        check("month-day-count", len(day_rows) == 4, str(day_text))
        check("month-order-desc", day_text[:4] == ["1.11.2026", "31.10.2026", "30.10.2026", "29.10.2026"], str(day_text))
        # Aggregat-Werte je Tag intakt
        alltxt = " ".join(rows)
        check("month-agg-values", "10.000" in alltxt and "11.000" in alltxt and "1.800" in alltxt, alltxt[:300])

        r = ok(await call("Page.captureScreenshot", {"format": "png"}))
        (OUT / "pt-p8-month-order.png").write_bytes(base64.b64decode(r["data"]))
        print("  shot:", OUT / "pt-p8-month-order.png")

    server.terminate()
    proc.terminate()
    print()
    print("PASS:", len(PASS), "FAIL:", len(FAIL))
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

if __name__ == "__main__":
    asyncio.run(main())
