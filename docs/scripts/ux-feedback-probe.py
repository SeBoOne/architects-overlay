#!/usr/bin/env python3
"""CDP-Probe für die Live-Feedback-Fixes (t_594c213c) auf feature/ux-feedback.

Belegt mit funktionalen DOM-Assertions (nicht nur Screenshots):
  1. Präfix-Case: ':C'-Eingabe parst wie ':c' (filters=commodity), Icons lowercase.
  2. Auto-Scroll: ArrowDown über den sichtbaren Bereich scrollt die Liste.
  3. Market-Modus: DAS Haupt-Suchfeld durchsucht Offers (kein eigenes .mq-Feld).
  4. Qualitätsfilter: Min/Max clientseitig, kombinierbar mit Sortierung.
  5. Tastatur: Alt+Zahl-Badges, ArrowDown/Enter/Esc, Tab erreicht Filter-Chips.

Datenquelle: smoke-stub.js (JS-Mirror des Provider-Kontrakts) — die Rust-Seite
ist durch cargo tests (crates/uex-client) belegt. Screenshots nach docs/screens/.
Wichtige Tasten (Tab/Enter) werden als TRUSTED CDP-Input-Events gesendet, damit
die Browser-Defaults (Fokus-Traversierung, Button-Aktivierung) greifen.
"""
import asyncio, base64, json, os, pathlib, sys
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens"
OUT.mkdir(parents=True, exist_ok=True)
URL = os.environ.get("PROBE_URL", "http://127.0.0.1:8795/")

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

async def main():
    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium",
        "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        "--remote-debugging-port=9225", "--user-data-dir=/tmp/cdp-ux2",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL,
    )
    import urllib.request
    ws = None
    for _ in range(60):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9225/json/list", timeout=1) as r:
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
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            v = r.get("result", {}).get("value")
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return v

        # Trusted keyboard input (native defaults: Tab-Traversierung, Button-Klick)
        # modifiers: Alt=1, Ctrl=2, Meta=4, Shift=8
        VK = {"Tab": (9, "Tab"), "Enter": (13, "Enter"), "Escape": (27, "Escape"),
              "ArrowDown": (40, "ArrowDown"), "ArrowUp": (38, "ArrowUp"),
              "ArrowLeft": (37, "ArrowLeft"), "ArrowRight": (39, "ArrowRight"),
              "m": (77, "KeyM")}
        for d in range(10):
            VK[str(d)] = (48 + d, "Digit" + str(d))

        async def key(k, alt=False, ctrl=False):
            """Trusted Input.dispatchKeyEvent. Enter braucht rawKeyDown+char('\\r')
            (Blink-Default: Button-Klick); Tab bewegt den Fokus nativ. Der erste
            trusted Tastendruck löst in headless einen WINFOCUS aus → der
            App-Fokus-Handler zieht den Fokus ins Suchfeld (eingenistetes
            Verhalten) — deshalb vor Enter/Buttons erst ein settle-Tab."""
            vk, code = VK[k]
            mods = (1 if alt else 0) | (2 if ctrl else 0)
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
                                                  "modifiers": mods})
            if k == "Enter":
                await call("Input.dispatchKeyEvent", {"type": "char", "key": k, "code": code,
                                                      "text": "\r", "unmodifiedText": "\r",
                                                      "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
                                                      "modifiers": mods})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
                                                  "modifiers": mods})
            await asyncio.sleep(0.18)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = base64.b64decode(r["data"])
            (OUT / name).write_bytes(data)
            print("  shot:", OUT / name, len(data), "bytes")

        def set_input_expr(sel, text):
            return (
                "(function(){"
                "var i=document.querySelector(" + json.dumps(sel) + ");"
                "if(!i) return 'NO-ELEMENT';"
                "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                "set.call(i,'');"
                "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));"
                "set.call(i," + json.dumps(text) + ");"
                "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                "return i.value;"
                "})()"
            )

        async def typeQuery(text, sel="input.q"):
            await js("var i=document.querySelector('input.q'); if(i) i.focus();")
            await js(set_input_expr(sel, text))
            await asyncio.sleep(0.7)  # Debounce 180 ms + invoke

        # invoke-Log instrumentieren (für Hide-Call-Prüfung)
        await js("""(function(){
            var orig = window.__TAURI_INTERNALS__.invoke;
            window.__calls = [];
            window.__TAURI_INTERNALS__.invoke = function(cmd,args){
                window.__calls.push(cmd + ':' + JSON.stringify(args || {}));
                return orig(cmd,args);
            };
        })()""")

        # ---- 1. Präfix-Case + lowercase Icons + Auto-Scroll -------------------
        await typeQuery(":C gold")
        check("prefix-uppercase-parses",
              await js("(globalThis.__lastSearch && globalThis.__lastSearch.filters && globalThis.__lastSearch.filters.join(',') === 'commodity')"),
              "lastSearch.filters=" + str(await js("globalThis.__lastSearch && globalThis.__lastSearch.filters || []")))
        vgroups = await js("[...document.querySelectorAll('.vgroup')].map(function(g){return g.textContent.replace(/\\s+/g,' ').trim()})")
        check("prefix-uppercase-shows-only-commodities",
              len(vgroups) == 1 and "commodities" in vgroups[0].lower(), str(vgroups))
        icons = await js("[...document.querySelectorAll('.row .ic')].map(function(x){return x.textContent}).join(',')")
        check("icons-lowercase",
              await js("(function(){var s=[...document.querySelectorAll('.row .ic')].map(function(x){return x.textContent}); return s.length>0 && s.slice(0,3).every(function(t){return t==='c'}) && !s.join('').includes('C');})()"),
              icons[:40])

        # Auto-Scroll: 'gold' (ca. 25 Treffer) + 15× ArrowDown (trusted)
        await typeQuery("gold")
        await asyncio.sleep(0.3)
        rows_total = await js("document.querySelectorAll('.row').length")
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        for _ in range(15):
            await key("ArrowDown")
        await asyncio.sleep(0.3)
        scroll = await js("document.querySelector('.list') ? document.querySelector('.list').scrollTop : -1")
        sel_idx = await js("[...document.querySelectorAll('.row')].findIndex(function(r){return r.classList.contains('is-sel')})")
        visible = await js("""(function(){
            var list=document.querySelector('.list'); if(!list) return '[]';
            var rows=document.querySelectorAll('.row');
            var lr=list.getBoundingClientRect();
            var out=[];
            for(var i=0;i<rows.length;i++){var r=rows[i].getBoundingClientRect();
              out.push(r.top>=lr.top-2 && r.bottom<=lr.bottom+2);}
            return JSON.stringify(out);
        })()""")
        vis = json.loads(visible)
        check("autoscroll-rows-overflow", rows_total >= 24, "rows=" + str(rows_total))
        check("autoscroll-scrolltop-moved", scroll > 0, "scrollTop=" + str(scroll))
        check("autoscroll-sel-visible", sel_idx >= 0 and vis[sel_idx] is True,
              "sel=" + str(sel_idx) + " sel_visible=" + str(vis[sel_idx]) + " scrollTop=" + str(scroll))
        await shot("09-search-lowercase-scroll.png")

        # ---- 2. Market-Modus: Haupt-Suchfeld + Badges + Helper ---------------
        await typeQuery("")
        await key("m", ctrl=True)
        await asyncio.sleep(0.9)
        check("market-no-own-search-field", await js("document.querySelector('.mq') === null") is True)
        ph = await js("document.querySelector('input.q').placeholder")
        check("market-placeholder-switched", "Marktplatz" in ph, ph)
        smode = await js("document.querySelector('.smode').innerText")
        check("market-smode", smode.strip() == "MARKETPLACE", smode)
        badges = await js("[...document.querySelectorAll('.mrow .mbadge')].map(function(b){return b.textContent}).join(',')")
        check("market-badges-1-0-first10", badges.startswith("1,2,3,4,5,6,7,8,9,0"), badges[:30])
        mrows = await js("document.querySelectorAll('.mrow').length")
        check("market-list-rows", mrows == 14, "rows=" + str(mrows))
        helper = await js("document.querySelector('.helper').textContent.replace(/\\s+/g,' ').trim()")
        check("market-helper-hints", "Alt+1..0" in helper and "Esc" in helper and "zurück zur Suche" in helper, helper[:100])
        chips_buttons = await js("document.querySelectorAll('.mbar button.chip, .mbar2 button.chip').length")
        check("market-filter-chips-are-buttons", chips_buttons >= 10,
              "buttons=" + str(chips_buttons))

        # Haupt-Suchfeld durchsucht Offers nach Terminal
        await typeQuery("tressler")
        last_q = await js("(globalThis.__lastMarket || {}).filters ? globalThis.__lastMarket.filters.q : null")
        mrows2 = await js("document.querySelectorAll('.mrow').length")
        check("market-main-field-query", last_q == "tressler" and mrows2 == 5,
              "q=" + str(last_q) + " rows=" + str(mrows2))
        await shot("07-market-main-search.png")

        # ---- 3. Qualitätsfilter (clientseitig, kombinierbar mit Sort) --------
        await js(set_input_expr("input.qnum", "900"))
        await asyncio.sleep(0.4)
        mrows3 = await js("document.querySelectorAll('.mrow').length")
        foot3 = await js("document.querySelector('.m-foot') ? document.querySelector('.m-foot').textContent.replace(/\\s+/g,' ').trim() : ''")
        check("quality-min-900-filters", mrows3 == 3, "rows=" + str(mrows3) + " foot=" + foot3)
        qmax = await js("[...document.querySelectorAll('.qnum')][1] ? [...document.querySelectorAll('.qnum')][1].value : 'none'")
        check("quality-max-empty-default", qmax == "", qmax)
        await shot("08-market-quality.png")

        # Sortierung per Tastatur: Tab (trusted) → nächster Fokus = Filter-Chip
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        await key("Tab")
        act = await js("document.activeElement ? document.activeElement.className + '|' + document.activeElement.textContent.trim() : ''")
        check("market-tab-reaches-chip", "chip" in act, act)
        # 'Preis ↑' per Keyboard aktivieren: fokussieren + trusted Enter
        await js("""(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()==='Preis ↑'}); if(b){b.focus();} return !!b;})()""")
        await key("Enter")
        await asyncio.sleep(0.8)
        sort_ok = await js("""(function(){
            var b=[...document.querySelectorAll('.chip')].find(function(x){return x.textContent.trim()==='Preis ↑'});
            return b ? b.classList.contains('is-on') : false;
        })()""")
        last_sort = await js("(globalThis.__lastMarket || {}).filters ? globalThis.__lastMarket.filters.sort : null")
        check("market-sort-keyboard-enter", sort_ok is True and last_sort == "price_asc",
              "is-on=" + str(sort_ok) + " sort=" + str(last_sort))
        # Qualität + Sortierung kombiniert: min 900 + Preis ↑ → erster = Gold Ore 4812
        first_price = await js("document.querySelector('.mrow .m-price') ? document.querySelector('.mrow .m-price').textContent : ''")
        check("quality-combines-with-sort", first_price.replace('.', '') == "4812" and ("4.812" in first_price or "4 812" in first_price), first_price)

        # ---- 4. Tastatur-Navigation Offers + Alt+Zahl + Esc ------------------
        await js(set_input_expr("input.qnum", ""))
        await asyncio.sleep(0.3)
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        await key("ArrowDown")
        await key("ArrowDown")
        sel_row = await js("[...document.querySelectorAll('.mrow')].findIndex(function(r){return r.classList.contains('is-sel')})")
        check("market-arrows-nav", sel_row == 2, "sel=" + str(sel_row))
        await key("2", alt=True)
        sel_row2 = await js("[...document.querySelectorAll('.mrow')].findIndex(function(r){return r.classList.contains('is-sel')})")
        check("market-alt-number-selects", sel_row2 == 1, "sel=" + str(sel_row2))
        await key("2", alt=True)
        act2 = await js("document.activeElement ? document.activeElement.className : ''")
        check("market-alt-number-again-focus-search", act2 == "q", str(act2))
        await key("Enter")
        await asyncio.sleep(0.3)
        detail_title = await js("document.querySelector('.mdetail h2') ? document.querySelector('.mdetail h2').textContent : ''")
        check("market-enter-opens-detail", len(detail_title) > 0, detail_title)
        await shot("10-market-detail.png")
        await key("Escape")
        in_market_after_esc = await js("document.querySelector('.mlist') !== null")
        check("market-esc-closes-detail-stays", in_market_after_esc is True)
        await key("Escape")
        await asyncio.sleep(0.8)
        smode2 = await js("document.querySelector('.smode') ? document.querySelector('.smode').innerText : 'NONE'")
        hides = await js("window.__calls ? window.__calls.filter(function(c){return c.indexOf('overlay_set_visible')>=0 && c.indexOf('false')>=0}).length : -1")
        check("market-esc-back-to-search", smode2.strip() == "SUCHE", smode2)
        check("market-esc-no-overlay-hide", hides == 0, "hide-calls=" + str(hides))

        # ---- 5. QA-Auflagen A/B: Location-Aggregat + Item-"nicht kaufbar" --
        # (t_cbd58b00): loadDetail() muss get_prices um id+kind erweitern.
        # Location-Call trägt kind:'location' + Panel zeigt Terminal-Gruppen;
        # Item-Call trägt kind:'item' + Panel zeigt "nicht kaufbar".
        async def last_price_call():
            calls = await js("window.__calls ? window.__calls.filter(function(c){return c.indexOf('get_prices:')>=0}) : []")
            return calls[-1] if calls else None

        # A: :l Checkmate öffnen → Location-Aggregat
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        await typeQuery(":l Checkmate")
        names = await js("[...document.querySelectorAll('.row .name')].map(function(n){return n.textContent})")
        check("loc-part5-search-shows-checkmate", any("Checkmate" in (n or "") for n in (names or [])), str(names)[:120])
        await js("document.querySelector('.row').click()")
        await asyncio.sleep(1.0)
        lp = await last_price_call()
        check("loc-part5-call-carries-kind-location", (lp or "").startswith('get_prices:{"commodity":"Commodity Shop - Admin - Checkmate"') and '"kind":"location"' in (lp or ""), str(lp)[:200])
        xpanA = await js("document.querySelector('.xpan') ? document.querySelector('.xpan').innerText.replace(/\\s+/g,' ').trim() : 'NONE'")
        print("    xpan-location:", xpanA[:200])
        check("loc-part5-panel-shows-terminal-group", "Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station" in (xpanA or ""), xpanA[:180])
        check("loc-part5-shows-per-terminal-rows", ("Admin - Checkmate" in (xpanA or "") or "keine Preisdaten" in (xpanA or "")), xpanA[:180])
        await shot("11-location-aggregate-checkmate.png")

        # B: :i CSP-68H öffnen → nicht kaufbar (keine Terminal-Tabellen)
        await typeQuery(":i CSP-68H")
        await js("document.querySelector('.row').click()")
        await asyncio.sleep(1.0)
        ip = await last_price_call()
        check("item-part5-call-carries-kind-item", (ip or "").startswith('get_prices:{"commodity":"CSP-68H Backpack"') and '"kind":"item"' in (ip or ""), str(ip)[:200])
        xpanB = await js("document.querySelector('.xpan') ? document.querySelector('.xpan').innerText.replace(/\\s+/g,' ').trim() : 'NONE'")
        print("    xpan-item:", xpanB[:200])
        check("item-part5-panel-not-purchasable", "nicht kaufbar" in (xpanB or ""), xpanB[:180])
        await shot("12-item-not-purchasable.png")

        console_events = console_events[:8]
        check("no-console-exceptions", len(console_events) == 0, str(console_events))

    print("\n===== RESULTAT =====")
    print("PASS:", len(PASS), " FAIL:", len(FAIL))
    for f in FAIL:
        print("  FAILED:", f)
    proc.terminate()
    return 1 if FAIL else 0

if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
