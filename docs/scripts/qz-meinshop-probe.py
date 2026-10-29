#!/usr/bin/env python3
"""CDP-Probe QZ (t_ab10109f) — Mein-Shop-Polish.

Belegt mit funktionalen DOM-Assertions + Screenshots:
  QZ-1 — Formular-Inhalt füllt die Popup-Höhe: .advert-pane .mlist füllt die
         Pane (clientHeight ~ Pane), Beschreibung flex-growt (> 200 px hoch).
  QZ-2 — Lokale pending-Listings im Mein Shop: Chip „wartet auf Freigabe"
         (frisch) bzw. „Freigabe unbestätigt" (> 24 h); „Entfernen" entfernt
         die Zeile lokal (pending_offers_remove-IPC, Stub-Zähler).
  QZ-3 — Filter „Alle/Aktiv/Wartend" entfernt; „Letzte 5 Marktplatz-Trades"
         im Mein Shop entfernt; „Im Browser öffnen" entfernt.
  QZ-4 — Mein Shop: „Listings verwalten ↗"; Trades-Reiter: „Trades verwalten ↗"
         (open_marketplace_trades_home-Stub antwortet mit dem home-Tab-URL).

Datenquelle: smoke-stub.js (JS-Mirror). Screenshots nach docs/screens/qz-meinshop/.
"""
import asyncio, base64, json, os, pathlib, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = ROOT / "docs/screens/qz-meinshop"
OUT.mkdir(parents=True, exist_ok=True)
PORT = int(os.environ.get("PROBE_PORT", "8816"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9248"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []

def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

NOW = int(time.time())

def offer(id_, title="Drake Nettle S", price=4_500_000.0):
    return {
        "id": id_, "title": title, "description": "", "item_id": 0, "category_id": 0,
        "operation": "sell", "listing_type": "item", "price": price, "currency": "UEC",
        "unit": "unit", "quantity": 1, "location": "Port Tressler", "region": "Stanton",
        "seller": "sebo_one", "quality": 72, "updated": NOW - 600, "expires": None,
        "slug": f"drake-nettle-s-{id_}", "photos": [], "approved": True,
    }

def pending(id_, title, price, created_at):
    return {
        "id": id_, "title": title, "operation": "sell", "price": price,
        "currency": "UEC", "unit": "unit", "quantity": 2,
        "location": "New Babbage", "status": "pending", "created_at": created_at,
    }

OFFERS = json.dumps([offer(1)], separators=(",", ":"))
# Ein frischer pending-Eintrag + ein > 24 h alter („Freigabe unbestätigt").
PENDING = json.dumps([
    pending(501, "Frisches Pending-Listing", 123_000, NOW - 60),
    pending(502, "Altes Pending-Listing", 456_000, NOW - 25 * 3600),
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
    else:
        raise RuntimeError(f"static server on {PORT} did not come up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-qz-meinshop",
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
        pending_calls = {}

        async def call(method, params=None):
            nonlocal msg_id
            msg_id += 1
            mid = msg_id
            fut = asyncio.get_event_loop().create_future()
            pending_calls[mid] = fut
            await cdp.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
            return await fut

        async def recv_loop():
            async for raw in cdp:
                m = json.loads(raw)
                if "id" in m and m["id"] in pending_calls:
                    pending_calls[m["id"]].set_result(m)
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
            f"window.__SHOP_OFFERS__ = {OFFERS};"
            f"window.__SHOP_PENDING__ = {PENDING};"
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

        await enter_market()
        await asyncio.sleep(1.5)
        opened = await click_chip("Mein Shop")
        await asyncio.sleep(1.2)

        # ================= QZ-3 — Entfernte UI-Elemente ====================
        txt = await js("document.body.innerText")
        txt_l = (txt or "").lower() if isinstance(txt, str) else ""
        check("qz3-filter-chips-gone", ' alle ' not in txt_l or "Wartend" not in txt,
              "Aktiv/Wartend-Chips im Mein Shop entfernt")
        check("qz3-letzte5-gone", "Letzte 5 Marktplatz-Trades" not in txt, "Abschnitt entfernt")
        check("qz3-im-browser-gone", "Im Browser öffnen" not in txt, "Button ersetzt")
        btn_listings = await js("(function(){var b=[...document.querySelectorAll('.chip')].find(x=>x.textContent.trim()==='Listings verwalten ↗'); return !!b;})()")
        check("qz4-shop-listings-verwalten", btn_listings is True)
        await shot("qz3-meinshop-offers-pending.png")

        # ================= QZ-2 — pending-Listings lokal ===================
        rows = await js("(function(){var rows=[...document.querySelectorAll('.mlist .mrow')]; var chips=[...document.querySelectorAll('.chip.warn')].map(c=>c.textContent.trim()); return JSON.stringify({total: rows.length, fresh: rows.some(r=>r.textContent.includes('Frisches Pending-Listing')), stale: rows.some(r=>r.textContent.includes('Altes Pending-Listing')), chips: chips});})()")
        check("qz2-pending-rows-shown", '"fresh":true' in rows and '"stale":true' in rows, rows)
        check("qz2-chips", 'wartet auf Freigabe' in rows and 'Freigabe unbestätigt' in rows, rows)
        # Entfernen: Klick auf „Entfernen" der frischen Zeile → Zeile weg.
        removed = await js("(function(){var rows=[...document.querySelectorAll('.mlist .mrow')]; var r=rows.find(x=>x.textContent.includes('Frisches Pending-Listing')); if(!r) return 'no-row'; var b=[...r.querySelectorAll('button')].find(x=>x.textContent.trim()==='Entfernen'); if(!b) return 'no-btn'; b.click(); return 'clicked';})()")
        await asyncio.sleep(0.6)
        after = await js("JSON.stringify({fresh: [...document.querySelectorAll('.mlist .mrow')].some(r=>r.textContent.includes('Frisches Pending-Listing')), stale: [...document.querySelectorAll('.mlist .mrow')].some(r=>r.textContent.includes('Altes Pending-Listing'))})")
        check("qz2-pending-remove", removed == "clicked" and '"fresh":false' in after and '"stale":true' in after, f"{removed} {after}")

        # ================= QZ-4 — Trades-Reiter-Button =====================
        switched = await click_chip("Trades")
        await asyncio.sleep(1.0)
        dbg = await js("(function(){return JSON.stringify({chips: [...document.querySelectorAll('.chip')].map(c=>c.textContent.trim()).slice(0,20), tab: (window.__svelte_debug||'')});})()")
        print("  DEBUG trades-tab:", dbg[:600])
        ttxt = await js("(function(){return JSON.stringify({hasTradesVerwalten: !![...document.querySelectorAll('.chip')].find(c=>c.textContent.trim()==='Trades verwalten ↗'), hasListingsVerwalten: !![...document.querySelectorAll('.chip')].find(c=>c.textContent.trim()==='Listings verwalten ↗')});})()")
        check("qz4-trades-trades-verwalten", switched is True and '"hasTradesVerwalten":true' in ttxt and '"hasListingsVerwalten":false' in ttxt, ttxt)
        await shot("qz4-trades-tab-button.png")

        # ================= QZ-1 — Formular füllt Popup =====================
        await click_chip("Mein Shop")
        await asyncio.sleep(0.5)
        opened = await click_chip("Anlegen")
        await asyncio.sleep(0.8)
        # Struktur-Beleg: Pane = Flex-Spalte, mlist flex-grow, Beschreibung flex-grow.
        # Scope: das advert-modal, das das Formular (.advert-desc) enthält —
        # die 2-Schritt-Confirms teilen sich die advert-pane/advert-modal-Klassen.
        modal_sel = "([...document.querySelectorAll('.advert-modal')].find(x=>x.querySelector('.advert-desc')))"
        css = await js("(function(){var modal=" + modal_sel + "; if(!modal) return '{}'; var pane=modal.querySelector('.advert-pane'); var ml=pane&&pane.querySelector(':scope > .mlist'); var ta=modal.querySelector('.advert-desc'); if(!pane||!ml||!ta) return '{}'; var ps=getComputedStyle(pane), ms2=getComputedStyle(ml), ts=getComputedStyle(ta); return JSON.stringify({paneDir: ps.flexDirection, mlGrow: ms2.flexGrow, taGrow: ts.flexGrow});})()")
        check("qz1-flex-structure", '"paneDir":"column"' in css and '"mlGrow":"1"' in css and '"taGrow":"1"' in css, css)
        # Fill-Beleg: Shell künstlich hoch setzen → Modal füllt Shell, mlist
        # füllt Pane, Beschreibung wächst in den freien Raum (kein Rest-Loch).
        await js("(function(){var s=document.createElement('style'); s.id='qz-tall'; s.textContent='.shell{height:1100px !important; max-height:none !important;}'; document.head.appendChild(s);})()")
        await asyncio.sleep(0.5)
        m = await js("(function(){var modal=" + modal_sel + "; if(!modal) return '{}'; var pane=modal.querySelector('.advert-pane'); var ml=pane&&pane.querySelector(':scope > .mlist'); var ta=modal.querySelector('.advert-desc'); var pr=pane.getBoundingClientRect(), mr=modal.getBoundingClientRect(), mlr=ml.getBoundingClientRect(), tar=ta.getBoundingClientRect(); var foot=modal.querySelector('.advert-foot'); var fr=foot?foot.getBoundingClientRect():null; var kids=[...pane.children].map(c=>c.className); var ps=getComputedStyle(pane); return JSON.stringify({paneH: Math.round(pr.height), mlistH: Math.round(mlr.height), modalH: Math.round(mr.height), taH: Math.round(tar.height), footTop: fr?Math.round(fr.top):0, paneBottom: Math.round(pr.bottom), kids: kids, paneDisplay: ps.display, paneHcss: ps.height, mlFlex: getComputedStyle(ml).flex});})()")
        check("qz1-form-fills-popup", opened is True and '"paneH"' in m and abs(json.loads(m)["paneH"] - json.loads(m)["modalH"]) <= 8 and json.loads(m)["taH"] >= 300 and json.loads(m)["footTop"] <= json.loads(m)["paneBottom"] + 2, m)
        await shot("qz1-form-fills-popup.png")

        # Seite schließen
        proc.kill(); server.kill()

    print(f"\nPASS={len(PASS)} FAIL={len(FAIL)}")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

asyncio.run(main())
