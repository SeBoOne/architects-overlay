#!/usr/bin/env python3
"""CDP-Probe QW (t_28bfd027) — Marktplatz-Fixes v0.5.0-Feedback.

Belegt mit funktionalen DOM-Assertions + Screenshots:
  QW-1 — Listing-Formular-Popup füllt die Shell (advert-modal Höhe == Shell
         nutzbare Höhe; Probe auf feature/mkt-fixes-2, Chromium-Render).
  QW-2 — Pfeiltasten LEAK: in Angebote öffnet → das Offer-Detail; in Trades
         tun ←/→ NICHTS (kein .mdetail, kein State-Wechsel).
  QW-4 — Abgeschlossener Deal (date_closed) → KEIN .chat-input/Composer,
         read-only-Hinweis + Abgeschlossen-Chip; offener Deal → Composer da.
  QW-5 — „Trade erfassen"-Popup komplett INNERHALB der Shell-Grenzen
         (Rect-Assertion gegen .shell — Regression zum fixed-Scrim-Bug).
  QW-6 — Tages-Aggregat über ALLE Trades (7 Fixtures an einem Tag →
         Ausgaben-Summe über alle 7, nicht nur 5; Backend-Pagination ist
         cargo-test-/Live-Beleg, hier die UI-Aggregat-Logik).

Datenquelle: smoke-stub.js (JS-Mirror). Screenshots nach docs/screens/qw-fixes/.
"""
import asyncio, base64, json, os, pathlib, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens/qw-fixes"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8811"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9243"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

NOW = int(time.time())

def negotiation(id_, closed=False):
    return {
        "id": id_, "listing_id": 4100 + id_, "hash": f"h-{id_}",
        "price": 1_000_000.0, "unit": "unit", "currency": "UEC",
        "deal_value": 990_000.0 if closed else None, "deal_value_currency": "UEC",
        "listing_title": f"Deal {id_}", "listing_slug": f"slug-{id_}",
        "advertiser_name": "A", "advertiser_username": "adv",
        "client_name": "C", "client_username": "cli",
        "is_listing_advertiser": True,
        "date_added": NOW - 86400, "date_modified": NOW - 60,
        "date_closed": (NOW - 3600) if closed else None,
    }

NEGOTIATIONS = json.dumps([negotiation(1, closed=False), negotiation(2, closed=True)], separators=(",", ":"))
MSGS = json.dumps({"h-2": [
    {"id": 1, "id_listing": 4102, "id_negotiation": 2, "event": None, "message": "Verlauf 1",
     "listing_title": "Deal 2", "listing_slug": "slug-2", "negotiation_hash": "h-2",
     "user_name": "Adv", "user_username": "adv", "api_name": "uex-web", "date_added": NOW - 7200, "date_read": None},
    {"id": 2, "id_listing": 4102, "id_negotiation": 2, "event": "negotiation_closed", "message": None,
     "listing_title": "Deal 2", "listing_slug": "slug-2", "negotiation_hash": "h-2",
     "user_name": "Adv", "user_username": "adv", "api_name": "uex-web", "date_added": NOW - 3600, "date_read": None},
]}, separators=(",", ":"))

# QW-6: 7 Trades am selben Tag, je 1000 SCU-price → Ausgabe 7 × 1000.
def trade(id_, op, price, date_added):
    return {"id": id_, "terminal_id": 21, "commodity_id": 11, "operation": op,
            "scu": 1, "price": price, "date_added": date_added,
            "commodity": "Gold", "terminal": "Everus Harbor",
            "vehicle": None, "fleet_name": None, "organization": None}
TRADES7 = json.dumps([trade(i, "buy", 1000, NOW - i) for i in range(1, 8)], separators=(",", ":"))

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

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-qw-fixes",
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
        EXTRA = (
            "window.__stubState.uex_api_key_set = true;"
            "window.__stubState.uex_secret_key_set = true;"
            "window.__stubState.uex_username = 'sebo_one';"
            f"window.__SHOP_NEGOTIATIONS__ = {NEGOTIATIONS};"
            f"window.__TRADE_MSGS__ = {MSGS};"
            f"window.__SHOP_TRADES__ = {TRADES7};"
        )
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": EXTRA})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        async def key(k, vk, code):
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.25)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            if "data" not in r:
                print("  shot FAILED (no data):", name)
                return
            (OUT / name).write_bytes(base64.b64decode(r["data"]))

        async def enter_market():
            await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
            await key("F2", 113, "F2")
            await asyncio.sleep(1.2)

        async def click_chip(sub):
            return await js(
                "(function(){var b=[...document.querySelectorAll('.chip')].find(x=>x.textContent.trim()===" + json.dumps(sub) + "); if(b){b.click(); return true;} return false;})()")

        ARROW_RIGHT = ("ArrowRight", 39, "ArrowRight")
        ARROW_LEFT = ("ArrowLeft", 37, "ArrowLeft")

        # ================= QW-2 — Pfeiltasten-Leak =========================
        await enter_market()
        await asyncio.sleep(1.5)
        # Angebote-Tab (default): → öffnet das Detail des ersten Angebots.
        await key(*ARROW_RIGHT)
        detail_open_offers = await js("!!document.querySelector('.mdetail')")
        check("qw2-offers-arrowright-opens", detail_open_offers is True)
        await key(*ARROW_LEFT)
        detail_closed = await js("!!document.querySelector('.mdetail')")
        check("qw2-offers-arrowleft-closes", detail_closed is False)
        # Trades-Reiter: ←/→ darf keinerlei Wirkung haben.
        await click_chip("Trades")
        await asyncio.sleep(1.0)
        await key(*ARROW_RIGHT)
        await asyncio.sleep(0.4)
        leaked_r = await js("!!document.querySelector('.mdetail')")
        await key(*ARROW_LEFT)
        await asyncio.sleep(0.4)
        state_leak = await js("JSON.stringify({detail: !!document.querySelector('.mdetail'), dealOpen: !!document.querySelector('.chat')})")
        check("qw2-trades-arrowright-noop", leaked_r is False, state_leak)
        await shot("qw2-trades-arrows-noop.png")

        # ================= QW-4 — Read-only Deals ==========================
        # Deal 1 (offen) → Composer da.
        open_btn = await js("(function(){var r=[...document.querySelectorAll('.mrow.trow')].find(x=>x.textContent.includes('Deal 1')); if(r){r.click(); return true;} return false;})()")
        await asyncio.sleep(1.0)
        composer_open = await js("!!document.querySelector('.chat-input input.rtext')")
        check("qw4-open-deal-composer", open_btn is True and composer_open is True)
        await js("(function(){var b=[...document.querySelectorAll('.chip')].find(x=>x.textContent.trim()==='← Deals'); if(b) b.click();})()")
        await asyncio.sleep(0.5)
        # Deal 2 (geschlossen) → kein Composer, read-only-Hinweis + Chip.
        closed_btn = await js("(function(){var r=[...document.querySelectorAll('.mrow.trow')].find(x=>x.textContent.includes('Deal 2')); if(r){r.click(); return true;} return false;})()")
        await asyncio.sleep(1.0)
        ro = await js("JSON.stringify({composer: !!document.querySelector('.chat-input'), note: document.body.innerText.toLowerCase().includes('verlauf nur lesbar'), chip: document.body.innerText.toLowerCase().includes('abgeschlossen'), history: document.querySelectorAll('.chat-msg').length})")
        check("qw4-closed-deal-readonly", closed_btn is True and '"composer":false' in ro and '"note":true' in ro and '"chip":true' in ro and '"history":1' in ro, "btn=%s %s" % (closed_btn, ro))
        await shot("qw4-closed-deal-readonly.png")

        # ================= QW-6 — Aggregat über alle Trades ================
        await js("(function(){var b=[...document.querySelectorAll('.chip')].find(x=>x.textContent.trim()==='← Deals'); if(b) b.click();})()")
        await asyncio.sleep(0.3)
        # Handelsrouten → Tracking (F3, dann Sub-Reiter).
        await key("F3", 114, "F3")
        await asyncio.sleep(1.2)
        await click_chip("Tracking")
        await asyncio.sleep(0.8)
        agg = await js("""
        (function(){
          var rows=[...document.querySelectorAll('.mlist .mrow')];
          var day=null;
          for (var r of rows){ if(r.textContent.includes('7 Trades')){ day=r.textContent.replace(/\\s+/g,' '); break; } }
          var foot=document.querySelector('.m-foot'); 
          return JSON.stringify({dayRow: day, foot: foot? foot.textContent.replace(/\\s+/g,' '): null});
        })()""")
        check("qw6-aggregate-covers-all-7", agg is not None and '"dayRow"' in str(agg) and "7 Trades" in str(agg), agg)
        await shot("qw6-aggregate-all-trades.png")

        # ================= QW-5 — Trade-erfassen-Popup in der Shell ========
        erfassen = await js("(function(){var b=[...document.querySelectorAll('.chip')].find(x=>x.textContent.includes('Trade erfassen')); if(b){b.click(); return true;} return false;})()")
        await asyncio.sleep(0.8)
        geo = await js("""
        (function(){
          var m=document.querySelector('.trk-modal'); var sh=document.querySelector('.shell');
          if(!m||!sh) return JSON.stringify({modal:null});
          var a=m.getBoundingClientRect(), b=sh.getBoundingClientRect();
          var foot=document.querySelector('.advert-foot');
          var f=foot? foot.getBoundingClientRect(): null;
          return JSON.stringify({modal:[a.x,a.y,a.width,a.height], shell:[b.x,b.y,b.width,b.height],
            inside: a.top>=b.top-1 && a.bottom<=b.bottom+1 && a.left>=b.left-1 && a.right<=b.right+1,
            footVisible: f? (f.bottom<=b.bottom+1): null, bodyScroll: (function(){var y=document.querySelector('.trk-body'); return y? [y.scrollHeight,y.clientHeight]:null;})()});
        })()""")
        g = json.loads(geo) if isinstance(geo, str) else {}
        check("qw5-trk-modal-inside-shell", erfassen is True and g.get("inside") is True and g.get("footVisible") is True, geo)
        await shot("qw5-trk-modal-inside-shell.png")
        await key("Escape", 27, "Escape")
        await asyncio.sleep(0.4)

        # ================= QW-1 — Advert-Popup füllt die Shell =============
        await key("F2", 113, "F2")
        await asyncio.sleep(1.0)
        await click_chip("Mein Shop")
        await asyncio.sleep(1.2)
        anlegen = await click_chip("Anlegen")
        await asyncio.sleep(0.8)
        ageo = await js("""
        (function(){
          var m=document.querySelector('.advert-modal'); var sh=document.querySelector('.shell'); var sc=document.querySelector('.advert-scrim');
          if(!m||!sh) return JSON.stringify({modal:null});
          var a=m.getBoundingClientRect(), b=sh.getBoundingClientRect();
          var pane=document.querySelector('.advert-pane');
          return JSON.stringify({modal:[a.x,a.y,a.width,a.height], shell:[b.x,b.y,b.width,b.height],
            fillsHeight: Math.abs(a.height-(b.height-28))<3,
            paneScrolls: pane? [pane.scrollHeight, pane.clientHeight]: null});
        })()""")
        g2 = json.loads(ageo) if isinstance(ageo, str) else {}
        check("qw1-advert-modal-fills-shell", anlegen is True and g2.get("fillsHeight") is True, ageo)
        await shot("qw1-advert-modal-full-height.png")

    print(f"\n== QW-Probe: {len(PASS)} PASS / {len(FAIL)} FAIL ==")
    if FAIL:
        print("FAILS:", ", ".join(FAIL))
        raise SystemExit(1)
    server.kill()
    proc.kill()

asyncio.run(main())
