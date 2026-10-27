#!/usr/bin/env python3
"""CDP-Probe für t_5b6d908f (RA: Routen-UI — Tab-Navigation fix, dynamischer
Sortier-Button, Profit-Absolut raus).

Belegt auf der echten Svelte-Oberfläche (echte Logik, Tauri-IPC gestubbt):
  P1: Tab wandert NATÜRLICH durch die Routen-Eingabefelder, die View wechselt
      dabei NICHT (kein keydown-Handler fängt Tab mehr ab für den
      Angebote⇄Routen-Switch). Shift+Tab geht rückwärts.
  P2: EIN einziger Gewinn/SCU-Schalter (3-Zustand: ↓ → ↑ → neutral), Richtung
      via Icon-Pfeil. Kein zweiter Gewinn/SCU-Chip mehr.
  P3: Die Sortierleiste zeigt KEINEN Profit-Absolut-Eintrag mehr; die übrigen
      Chips sind Marge ↓ und Distanz ↑.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "evidence-live-smoke/routes-sorting-ui")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8796/"

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8796", "--bind", "127.0.0.1",
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
        server.terminate()
        raise RuntimeError("static server did not come up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1600,1000", "--hide-scrollbars",
        "--remote-debugging-port=9237", "--user-data-dir=/tmp/cdp-routes-sorting-ui",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9237/json/list", timeout=1) as r:
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
        await call("Emulation.setDeviceMetricsOverride", {"width": 1600, "height": 1000, "deviceScaleFactor": 1, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.5)

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        VK = {"F2": 113, "F6": 117, "Escape": 27, "Enter": 13, "Tab": 9,
              "ArrowDown": 40, "ArrowUp": 38}
        async def key(k, shift=False):
            vk = VK[k]
            mods = 8 if shift else 0  # CDP: 8 = Shift
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
                                                  "modifiers": mods})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
                                                  "modifiers": mods})
            await asyncio.sleep(0.35)

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            (OUT / name).write_bytes(base64.b64decode(r["data"]))
            print("  shot:", OUT / name)

        # ---- Nach Market (F2) in den Routen-Tab wechseln: per Klick (NICHT Tab —
        #      genau das ist der zu belegende Punkt). ----
        await key("F2")
        await asyncio.sleep(1.0)
        tabs = await js("!!document.querySelector('.mtabs')")
        check("market-opened-mtabs", bool(tabs))
        # Handelsrouten-Chip klicken (Tab-Switch wäre entfernt → Klick nötig).
        await js("([...document.querySelectorAll('.mtabs button')].find(b => b.textContent.includes('Handelsrouten'))||{}).click ? ([...document.querySelectorAll('.mtabs button')].find(b => b.textContent.includes('Handelsrouten'))).click() : null")
        await asyncio.sleep(1.4)
        rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        check("routes-list-rendered", rows == 3, "rows=" + str(rows))
        routes_active = await js("([...document.querySelectorAll('.mtabs button')].find(b => b.textContent.includes('Handelsrouten'))||{}).className")
        check("routes-tab-active", bool(routes_active) and "is-on" in str(routes_active), routes_active)

        # ---- P3: Sortierleiste zeigt KEINEN Profit-Absolut mehr; nur EIN Gewinn/SCU + Marge + Distanz ----
        # Nur die mbar2 mit dem "Sort"-Label (Routen-Sortierleiste) → deren Chips.
        sort_labels = json.loads(await js("JSON.stringify([...document.querySelectorAll('.mbar2')].find(m => m.textContent.includes('Sort')) ? [...([...document.querySelectorAll('.mbar2')].find(m => m.textContent.includes('Sort')).querySelectorAll('button.chip'))].map(b=>b.textContent.trim()) : [])"))
        check("no-profit-absolut", not any("Profit absolut" in s for s in sort_labels), str(sort_labels))
        gewinn_buttons = [s for s in sort_labels if "Gewinn/SCU" in s]
        check("single-gewinne-button", len(gewinn_buttons) == 1, "count=" + str(len(gewinn_buttons)))
        other_sorts = [s for s in sort_labels if "Gewinn" not in s]
        check("marge-dist unchanged", sorted(s.strip() for s in other_sorts) == sorted(["Marge ↓", "Distanz ↑"]), str(other_sorts))
        await shot("01-routes-sortbar-single-gewinne-no-profit.png")

        # ---- P2: 3-Zustand-Durchschalten ↓ → ↑ → neutral (→ ↓) ----
        gsel = "([...document.querySelectorAll('.mbar2')].find(m => m.textContent.includes('Sort')).querySelectorAll('button.chip'))"
        gidx = "([...([...document.querySelectorAll('.mbar2')].find(m => m.textContent.includes('Sort')).querySelectorAll('button.chip'))].findIndex(b => b.textContent.includes('Gewinn/SCU')))"
        async def gewinne_text():
            return await js(f"(({gsel}))[{gidx}].textContent.trim()")
        d0 = await gewinne_text()            # Default profit_desc → ↓
        check("state-1-default-desc", "↓" in d0, d0)
        # zuerst auf ↑ (aufsteigend)
        await js(f"(({gsel}))[{gidx}].click()")
        await asyncio.sleep(0.9)
        d1 = await gewinne_text()
        check("state-2-asc", "↑" in d1 and "↓" not in d1, d1)
        await shot("02-gewinne-asc.png")
        # neutral (kein Pfeil)
        await js(f"(({gsel}))[{gidx}].click()")
        await asyncio.sleep(0.9)
        d2 = await gewinne_text()
        check("state-3-neutral-no-arrow", "↓" not in d2 and "↑" not in d2, d2)
        await shot("03-gewinne-neutral.png")
        # zurück auf ↓
        await js(f"(({gsel}))[{gidx}].click()")
        await asyncio.sleep(0.9)
        d3 = await gewinne_text()
        check("state-4-back-desc", "↓" in d3, d3)
        await shot("04-gewinne-desc.png")

        # ---- P1: Tab wandert durchs Formular, View bleibt ----
        await js(f"document.querySelector('[aria-label^=\"Schiff\"]').focus()")
        await asyncio.sleep(0.3)
        view_unchanged = True
        seq = []
        for i in range(14):
            await key("Tab")
            info = await js("(function(){var a=document.activeElement; if(!a) return 'none';"
                            "var lbl=a.getAttribute && a.getAttribute('aria-label')||'';"
                            "var ph=a.getAttribute && a.getAttribute('placeholder')||'';"
                            "var txt=(a.textContent||'').trim();"
                            "return (a.tagName||'')+'|'+(ph||lbl||txt||'').slice(0,28);})()")
            routes_on = await js("([...document.querySelectorAll('.mtabs button')].find(b => b.textContent.includes('Handelsrouten')||b.textContent.includes('Routen'))||{}).className")
            offers_on = await js("([...document.querySelectorAll('.mtabs button')].find(b => b.textContent.includes('Angebote'))||{}).className")
            if "is-on" in str(offers_on):
                view_unchanged = False
            seq.append(info)
            if i == 4:
                await shot("05-tab-mid-form.png")
        check("tab-view-never-switches", view_unchanged, "offers never got is-on")
        # mind. 2 verschiedene Eingabefelder durchlaufen (natürliche Tab-Nav)
        input_hits = [s for s in seq if s.startswith("INPUT")]
        check("tab-moves-through-inputs", len(set(input_hits)) >= 2, "inputs=" + str(sorted(set(input_hits))))
        print("   tab sequence:", seq)

        # Shift+Tab rückwärts: SCU-Feld fokussieren, Shift+Tab → Ware-Vorgänger
        await js(f"document.querySelector('[placeholder=\"SCU\"]').focus()")
        await asyncio.sleep(0.3)
        before = await js("document.activeElement.placeholder")
        await key("Tab", shift=True)
        await asyncio.sleep(0.3)
        after = await js("document.activeElement.placeholder || (document.activeElement.tagName||'') + ':' + (document.activeElement.textContent||'').trim().slice(0,12)")
        check("shift-tab-backward-moves", before == "SCU" and after != "SCU", f"from SCU -> {after}")
        await shot("06-shift-tab-backward.png")

        # View immer noch Routen (robust: Angebote-Chip NICHT is-on + Liste da)
        offers_on2 = await js("([...document.querySelectorAll('.mtabs button')].find(b => b.textContent.includes('Angebote'))||{}).className")
        still_rows = int(await js("document.querySelectorAll('[data-route-idx]').length"))
        routes_btn2 = await js("JSON.stringify([...document.querySelectorAll('.mtabs button')].map(b=>b.textContent.trim()+'='+(b.className||'')))")
        check("view-still-routes-after-tab", "is-on" not in str(offers_on2) and still_rows > 0,
              f"offers={offers_on2} rows={still_rows} tabs={routes_btn2}")

    server.terminate(); proc.terminate()

    print("\n=== RESULT ===")
    for p in PASS: print("  PASS", p)
    for f in FAIL: print("  FAIL", f)
    print(f"{len(PASS)} passed, {len(FAIL)} failed")
    raise SystemExit(1 if FAIL else 0)

if __name__ == "__main__":
    asyncio.run(main())