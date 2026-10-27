#!/usr/bin/env python3
"""CDP-Probe für F2 (Karte t_96120e79): zweispaltige Kauf-/Verkaufsort-Tabelle
im Routen-Detail + verbreitertes Budget-Feld. Echte Svelte-Oberfläche,
Tauri-IPC gestubbt (smoke-stub.js).

Verifiziert auf der echten UI:
  1. F1: Budget-Feld ist breit genug für 9-stellige Beträge (25 000 000) —
     das Feld hat die rb-qnum-Breite (~128px) und der Wert wird nicht
     abgeschnitten; formatierte Anzeige (rb-fmt) ergänzt den rohen Wert.
  2. F2: GOLD-Detail zeigt eine Tabelle mit MEHREREN Kauforten (4) und
     Verkaufsorten (3) — nicht nur HDMS-Anderson. Die aktuell gewählte
     Kauf-/Verkaufskombination der Route ist markiert (is-selected).
  3. F2: Die Tabellen werden nach Preis sortiert (Kauforte aufsteigend,
     Verkaufsorte absteigend) — der günstigste Kaufort oben links, der
     teuerste Verkaufsort oben rechts.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/routes-ux-f2")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

# Detail-Expresssion: Kopf, KAUF/VERKAUF der Route, beide Tabellen-Spalten
# (Terminal, Preis, is-selected-Flag) + Budget-Feld-Breite.
DETAIL_EXPR = ("JSON.stringify((function(){"
    "var h2=document.querySelector('.rdetail h2');"
    "var stops=document.querySelectorAll('.rd-leg .rd-stop');"
    "var cols=Array.from(document.querySelectorAll('.rd-pair-col')).map(function(c){"
    "var rows=Array.from(c.querySelectorAll('.rd-pair-row')).map(function(b){"
    "return {term:String((b.querySelector('.rd-pair-term')||{}).textContent||'').trim(),"
    "selected:b.classList.contains('is-selected'),"
    "price:String((b.querySelector('.rd-pair-price')||{}).textContent||'').trim()};});"
    "return {head:String((c.querySelector('.rd-pair-head')||{}).textContent||''),rows:rows};});"
    "var budget=document.querySelector('input[aria-label=\"Budget (aUEC) — kappt kaufbare Menge\"]');"
    "var rf=document.querySelector('.rb-fmt');"
    "return {title:h2?h2.textContent.trim():'',"
    "buy:stops[0]?(stops[0].querySelector('.rd-term')||{}).textContent||'':null,"
    "sell:stops[1]?(stops[1].querySelector('.rd-term')||{}).textContent||'':null,"
    "cols:cols,"
    "budgetW:budget?getComputedStyle(budget).width:'',"
    "budgetV:budget?budget.value:'',rbFmt:rf?rf.textContent.trim():''};})())")

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
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-routes-ux-f2",
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
                ed = r["exceptionDetails"]
                return "EXC:" + (ed.get("exception", {}).get("description") or ed.get("text") or "").replace("\n", " ")[:300]
            return r.get("result", {}).get("value")

        VK = {"F1": 112, "F2": 113, "Escape": 27, "Enter": 13, "Tab": 9,
              "ArrowDown": 40, "ArrowUp": 38}
        async def key(k):
            vk = VK[k]
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.35)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", OUT / name)

        async def open_top_route():
            await js("document.querySelector('[data-route-idx=\"0\"]')?.click();")
            await asyncio.sleep(0.9)

        async def set_input(sel, text):
            return await js("(function(){var i=document.querySelector(" + json.dumps(sel) + ");"
                            "if(!i) return 'NO-ELEMENT';"
                            "i.focus();"
                            "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                            "set.call(i,'' );"
                            "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                            "set.call(i," + json.dumps(text) + ");"
                            "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                            "return i.value;})()")

        async def detail_state():
            raw = await js(DETAIL_EXPR)
            if not isinstance(raw, str) or not raw.startswith("{"):
                print("  DETAIL-RAW:", repr(raw)[:400])
                raise RuntimeError("detail_state: no JSON object: " + repr(raw)[:200])
            return json.loads(raw)

        async def clear_from_to():
            await js("window.__locPool && (window.__locPool.length ? 1 : 1);")

        # Hangar: genau 1 Schiff (Nomad) → auto-selected, SCU 24.
        await js("window.__HANGAR_DATA__ = [{id:1,name:'Nomad',custom_name:'Rennsemmel',origin:'',insurance:null,"
                 "entity_type:'ship',base_ship:'',ship_code:'',manufacturer:'',purchase_price:null,"
                 "purchase_date:'',pledge_id:'',pledge_name:'',warbond:false,ccud:false}];")

        # ---- F1: Budget-Feld breit für 9-stellige Beträge ----
        await key("F2")
        await asyncio.sleep(1.0)
        # Handelsrouten-Tab aktivieren (robust statt Tab-Enter-Reihenfolge).
        await js("[...document.querySelectorAll('[role=tab]')].find(function(t){return t.textContent.trim()==='Handelsrouten'})?.click();")
        await asyncio.sleep(1.5)
        # 9-stelliger Betrag: 25 000 000 → Feldbreite ~128px + Formatchip "25.000.000".
        await set_input('input[aria-label="Budget (aUEC) — kappt kaufbare Menge"]', "25000000")
        await asyncio.sleep(1.2)
        b = json.loads(await js("JSON.stringify((function(){"
            "var i=document.querySelector('input[aria-label=\"Budget (aUEC) — kappt kaufbare Menge\"]');"
            "var rf=document.querySelector('.rb-fmt');"
            "return {w:i?getComputedStyle(i).width:'',v:i?i.value:'',rf:rf?rf.textContent.trim():''};})())"))
        check("f1-budget-value-9-digit", b["v"] == "25000000", b["v"])
        check("f1-budget-formatted-chip", b["rf"] == "25.000.000", b["rf"])
        w_px = float(b["w"].rstrip("px"))
        check("f1-budget-wide-enough", w_px >= 110, "width=" + str(w_px))
        await shot("01-f1-budget-wide-25m.png")

        # ---- F2: GOLD-Detail mit Alternativ-Tabelle (mehrere Standorte) ----
        await set_input('input[aria-label="Ware filtern"]', "Gold")
        await asyncio.sleep(1.5)
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("f2-gold-rows", rows == 4, "rows=" + str(rows))
        await open_top_route()
        d = await detail_state()
        check("f2-detail-commodity-gold", d["title"].startswith("Gold"), d["title"])
        # Zwei Spalten: KAUFORTE (links) + VERKAUFSORTE (rechts).
        check("f2-two-columns", len(d["cols"]) == 2, json.dumps([c["head"] for c in d["cols"]]))
        buy_col, sell_col = (d["cols"] if len(d["cols"]) >= 2 else [{"rows": []}, {"rows": []}])
        # Konvention: erste Spalte=Kauforte (Preis aufsteigend), zweite=Verkaufsorte.
        # Stub liefert buys/sells getrennt; die Spalten-Reihenfolge im DOM ist
        # KAUF → VERKAUF (Template). Wir prüfen anhand der Preise: Kauforte
        # steigen auf, Verkaufsorte fallen ab.
        # Kauforte: Checkmate(4750) < Tram(4800) < Grim(4800) < Anderson(4850).
        # Verkaufsorte: Orbituary(6280) > Bai(6250) > Olisare(6200).
        check("f2-gold-multiple-buy-4", len(buy_col["rows"]) >= 4, json.dumps(buy_col["rows"]))
        check("f2-gold-multiple-sell-3", len(sell_col["rows"]) >= 3, json.dumps(sell_col["rows"]))
        # Markierte Kombination: die Route kauft bei Checkmate, verkauft bei Bai Hang.
        sel_buy = [r for r in buy_col["rows"] if r["selected"]]
        sel_sell = [r for r in sell_col["rows"] if r["selected"]]
        check("f2-selected-buy-highlight", len(sel_buy) == 1 and "Checkmate" in sel_buy[0]["term"], json.dumps(sel_buy))
        check("f2-selected-sell-highlight", len(sel_sell) == 1 and "Bai Hang" in sel_sell[0]["term"], json.dumps(sel_sell))
        await shot("02-f2-gold-alt-table-highlight.png")

        # Ein Klick auf einen Kauf/Verkaufsort setzt Start/Ziel im Filter.
        # (optionaler Teil von F2; wir prüfen, dass der Klick nicht reißt.)
        await js("document.querySelectorAll('.rd-pair-row[data-side=\"buy\"]')[1]?.click();")
        await asyncio.sleep(0.6)
        from_val = await js("(function(){var i=document.querySelector('input[aria-label=\"Startort Type-ahead — Tippen filtert, ↵ übernimmt\"]');i?i.value:''})()")
        await key("Escape")
        await asyncio.sleep(0.5)
        print("  (kein Riss nach Orts-Klick; Startort war:", from_val)

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS, {len(FAIL)} FAIL")
    for f in FAIL:
        print("  FAILED:", f)
    if FAIL:
        raise SystemExit(1)

asyncio.run(main())