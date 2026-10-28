#!/usr/bin/env python3
"""Temporaerer Ghost-Button-Computed-Style-Probe (t_073ee278) — NICHT committen.

Misst getComputedStyle().backgroundColor auf ALLEN echten <button class="btn-ghost">:
1. 'In den Einstellungen hinterlegen' (Secret-Key-Fall, uex_secret_key_set=true)
2. 'Neu'/'Abmelden' in der Listing-Liste
3. Confirm-Modal 'Abbrechen' (nach Abmelden-Klick -> 2-Schritt-Modal)
Zusaetzlich: .btn Primary bleibt Akzent, .btn-ghost.btn-danger bleibt rot getoent.
"""
import asyncio, base64, json, os, pathlib, sys, time
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
BUILD = ROOT / "build"
PORT = int(os.environ.get("PROBE_PORT", "8798"))
CDP_PORT = int(os.environ.get("PROBE_CDP_PORT", "9232"))
URL = f"http://127.0.0.1:{PORT}/"

PASS, FAIL = [], []
def check(name, okv, detail=""):
    (PASS if okv else FAIL).append(name)
    print(("  PASS " if okv else "  FAIL ") + name + ((" — " + detail) if detail else ""))

STUB_EXTRA = (
    "window.__stubState.uex_api_key_set = true;"
    "window.__stubState.uex_secret_key_set = true;"
    "window.__stubState.uex_username = 'sebo_one';"
    "localStorage.setItem('architects-overlay.shop.username','sebo_one');"
    "window.__stubState.marketplace_confirm = 'every_action';"
)

def offer(id_, title, location, op, price, qty):
    return {"id": id_, "title": title, "description": "", "item_id": id_, "category_id": 1,
            "operation": op, "listing_type": "item", "price": price, "currency": "UEC",
            "unit": "scu", "quantity": qty, "location": location, "region": "Stanton",
            "seller": "sebo_one", "quality": 850, "updated": int(time.time()) - 3600,
            "expires": None, "slug": f"sebo-{id_}"}

SHOP_OFFERS = json.dumps([offer(1, "Gold Ore (Raw)", "Port Tressler", "sell", 4812, 1200)], separators=(",", ":"))
STUB_EXTRA += "window.__SHOP_OFFERS__ = " + SHOP_OFFERS + ";"

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
        "--directory", str(BUILD),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    import urllib.request
    for _ in range(60):
        try:
            with urllib.request.urlopen(URL, timeout=1) as r:
                if r.status == 200: break
        except Exception: pass
        await asyncio.sleep(0.25)
    print("static server up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1600", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-ghost-probe",
        "about:blank", stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{CDP_PORT}/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]; break
        except Exception: pass
        await asyncio.sleep(0.25)
    if ws is None:
        raise RuntimeError("chromium CDP not up")

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
                m = json.loads(raw)
                if "id" in m and m["id"] in pending: pending[m["id"]].set_result(m)
        asyncio.create_task(recv_loop())
        def ok(r):
            if "error" in r: raise RuntimeError("CDP error: %s" % r["error"])
            return r.get("result", {})

        await call("Page.enable"); await call("Runtime.enable")
        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + str(r["exceptionDetails"].get("text"))
            return r.get("result", {}).get("value")

        async def shot(name, outdir):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (outdir / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", name)

        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB_EXTRA})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.2)

        async def key(k, vk, code):
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.2)

        async def click_chip(label):
            return await js("(function(){var b=[...document.querySelectorAll('.chip')].find(function(x){return (x.textContent||'').trim()===" + json.dumps(label) + "}); if(b){b.click(); return true;} return false;})()")

        async def enter_shop_open_form():
            await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
            await key("F2", 113, "F2")
            await asyncio.sleep(1.0)
            await click_chip("Mein Shop")
            await asyncio.sleep(1.5)
            await click_chip("Anlegen")
            await asyncio.sleep(0.8)

        OUT = ROOT / "docs/screens/ghost-fix"
        OUT.mkdir(parents=True, exist_ok=True)

        async def nav(extra=""):
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
            await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB_EXTRA + extra})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.2)

        # State 1: Secret-Key-Case — uex_secret_key_set=false, Mein-Shop-Reiter
        await nav(";window.__stubState.uex_secret_key_set = false;")
        await js("(function(){var i=document.querySelector('input.q'); if(i) i.focus();})()")
        await key("F2", 113, "F2")
        await asyncio.sleep(1.0)
        await click_chip("Mein Shop")
        await asyncio.sleep(1.2)
        hint_btn = await js("""(function(){
            var b=[...document.querySelectorAll('button.btn-ghost')].find(function(x){
                return (x.textContent||'').includes('Einstellungen hinterlegen') && !!x.offsetParent;});
            if(!b) return null; var c=getComputedStyle(b);
            return {bg:c.backgroundColor, fg:c.color, visible:!!b.offsetParent};})()""")
        check("secret-key-hint-button-transparent",
              bool(hint_btn) and hint_btn["bg"] == "rgba(0, 0, 0, 0)", json.dumps(hint_btn))
        await shot("ghost-secretkey-hint.png", OUT)

        # State 2: mit Secret-Key — Anlegen-Formular + Neu/Abmelden
        await nav()
        await enter_shop_open_form()
        ghosts = await js("""(function(){
            return [...document.querySelectorAll('button.btn-ghost')].map(function(b){
                var c = getComputedStyle(b);
                return {label:(b.textContent||'').trim().slice(0,40),
                        bg:c.backgroundColor, fg:c.color,
                        visible: !!b.offsetParent, danger: b.classList.contains('btn-danger')};
            });
        })()""")
        print(json.dumps(ghosts, indent=1))
        real = [g for g in (ghosts or []) if g.get("visible")]
        non_trans = [g for g in real if g["bg"] != "rgba(0, 0, 0, 0)"]
        ua_gray = [g for g in real if "rgb(107" in g["bg"]]
        check("all-visible-button-ghost-transparent", bool(real) and not non_trans and not ua_gray,
              "nontransparent=" + json.dumps(non_trans))
        # Danger-Ghost bleibt rot getoent (nicht transparent)
        dangers = [g for g in real if g["danger"]]
        check("danger-ghost-red-tint", all(g["bg"] != "rgba(0, 0, 0, 0)" for g in dangers), json.dumps(dangers))
        # Primary .btn bleibt Akzent
        prim = await js("""(function(){
            var b=[...document.querySelectorAll('button.btn')].find(function(x){return (x.textContent||'').includes('Anlegen')});
            if(!b) return null; var c=getComputedStyle(b);
            return {bg:c.backgroundColor, fg:c.color};})()""")
        check("primary-btn-accent", bool(prim) and prim["bg"] not in ("rgba(0, 0, 0, 0)", "rgb(107, 107, 107)"), json.dumps(prim))
        await shot("ghost-shop-form.png", OUT)

        # State 3: Abmelden-Klick -> Confirm-Modal -> Abbrechen-Button
        await click_chip("Listings verwalten")
        await asyncio.sleep(1.2)
        clicked = await js("""(function(){
            var b=[...document.querySelectorAll('button.btn-ghost')].find(function(x){return (x.textContent||'').trim()==='Abmelden'});
            if(b){b.click(); return true;} return false;})()""")
        await asyncio.sleep(0.8)
        modal = await js("""(function(){
            var b=[...document.querySelectorAll('button.btn-ghost')].find(function(x){
                return (x.textContent||'').trim()==='Abbrechen' && !!x.offsetParent;});
            if(!b) return null; var c=getComputedStyle(b);
            return {bg:c.backgroundColor, fg:c.color, visible:!!b.offsetParent};})()""")
        check("cancel-clicked-modal-open", bool(clicked) and bool(modal), "clicked=%s modal=%s" % (clicked, json.dumps(modal)))
        check("modal-cancel-btn-ghost-transparent",
              bool(modal) and modal["bg"] == "rgba(0, 0, 0, 0)",
              json.dumps(modal))
        await shot("ghost-confirm-modal.png", OUT)

        # Cleanup: Modal schliessen
        await js("(function(){var b=[...document.querySelectorAll('button.btn-ghost')].find(function(x){return (x.textContent||'').trim()==='Abbrechen'&&!!x.offsetParent}); if(b)b.click();})()")

    print("\nSUMMARY: %d PASS / %d FAIL" % (len(PASS), len(FAIL)))
    if FAIL: print("FAILED:", FAIL)
    sys.exit(1 if FAIL else 0)

asyncio.run(main())
