#!/usr/bin/env python3
"""CDP-Probe für t_5fe8eb4c (K2: Overlay-Layout — feste Breite, Max-Höhe, Skalierung).

Datenquelle: smoke-stub.js (IPC-Grenze gestubbt, echte Svelte-UI). Checks:

  P4 — feste Breite:
   1. Shell-Breite IDENTISCH in allen Ansichten (F1 Suche, F2 Markt, F6 Routen,
      F3 Inventar, F4 Hangar, F12 Settings) und == min(max(70vw,640px),94vw).
   2. Kein horizontaler Overflow (scrollWidth <= innerWidth) in keiner Ansicht.

  P3 — Max-Höhe + inneres Scrollen:
   3. Lange Liste ("gold" → 27 Treffer): .list scrollt INNEN (scrollHeight >
      clientHeight, scrollTop bewegbar), Shell-Unterkante bleibt ≤ 92 vh.
   4. Hangar mit 30 Schiffen: .hangar-list scrollt innen, Shell bleibt im
      Viewport (bottom < viewportHeight).

  P12 — Schrift-Skalierung:
   5. F12 → Settings-View zeigt Chips 80/90/100/110/125 %.
   6. Chip 80 %: --ui-scale = 0.8 auf <html>, Einstellungen live (ohne Reload);
      Chip 125 %: --ui-scale = 1.25; visuelle Größe skaliert (Probe-Element
      wächst/schrumpft), Außenbox (Shell-Visual-Breite) bleibt konstant.
   7. settings_set_scale IPC wird gefeuert (Stub-Spiegel scale == gewählt).

  Regression:
   8. Enter-Detail-Filter: Enter auf Suchergebnis öffnet Detail (in-detail),
      Esc zurück; doppeltes Esc löscht Query.
   9. Rail-Kollaps bei schmaler Viewport (480 px): .rail.is-compact aktiv;
      bei 1920 px nicht kompakt.

Screenshots je Ansicht (feste Breite sichtbar), Scroll, Skalierung 80 %/125 %.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/layout-sizing")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8798/"
VW, VH = 1920, 1080

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8798", "--bind", "127.0.0.1",
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
        f"--window-size={VW},{VH}", "--hide-scrollbars",
        "--remote-debugging-port=9235", "--user-data-dir=/tmp/cdp-layout-sizing",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9235/json/list", timeout=1) as r:
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

    try:
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
            await call("Emulation.setDeviceMetricsOverride",
                       {"width": VW, "height": VH, "deviceScaleFactor": 2, "mobile": False})
            await call("Page.addScriptToEvaluateOnNewDocument",
                       {"source": STUB + "\nwindow.__HANGAR_DATA__ = Array.from({ length: 30 }, (_, i) => ({ id: i + 1, name: 'Ship ' + String(i + 1).padStart(2, '0'), origin: i % 2 ? 'ingame' : 'pledge', entity_type: 'ship', modules: [], insurance: i % 3 ? 'LTI' : 'Std' }));"})
            await call("Page.navigate", {"url": URL})
            await asyncio.sleep(2.5)

            async def js(expr):
                r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
                if r.get("exceptionDetails"):
                    return "EXC:" + (r["exceptionDetails"].get("text") or "")
                return r.get("result", {}).get("value")

            VK = {"F1": 112, "F2": 113, "F3": 114, "F4": 115, "F6": 117, "F12": 123,
                  "Escape": 27, "Enter": 13, "g": 71, "o": 79, "l": 76, "d": 68}
            async def key(k, mods=()):
                vk = VK[k]
                for m in mods:
                    await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": m, "code": m,
                                                          "windowsVirtualKeyCode": VK[m], "nativeVirtualKeyCode": VK[m]})
                await call("Input.dispatchKeyEvent", {"type": "keyDown", "key": k, "code": k,
                                                      "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk, "text": k if len(k) == 1 else ""})
                await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                      "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
                for m in mods:
                    await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": m, "code": m,
                                                          "windowsVirtualKeyCode": VK[m], "nativeVirtualKeyCode": VK[m]})
                await asyncio.sleep(0.35)

            async def shot(name):
                r = ok(await call("Page.captureScreenshot", {"format": "png"}))
                (OUT / name).write_bytes(base64.b64decode(r["data"]))
                print("  shot:", OUT / name)

            async def shellMetrics():
                return await js("""(() => {
                    const s = document.querySelector('.shell');
                    const r = s ? s.getBoundingClientRect() : null;
                    return r ? {
                        w: Math.round(r.width), h: Math.round(r.height),
                        bottom: Math.round(r.bottom), right: Math.round(r.right),
                        left: Math.round(r.left)
                    } : null;
                })()""")

            async def typeIntoSearch(text):
                for ch in text:
                    await key(ch)
                await asyncio.sleep(0.4)

            expectedW = int(min(max(0.70 * VW, 640), 0.94 * VW))
            modes = [("F1", "search", "01-f1-search"), ("F2", "market", "02-f2-market"),
                     ("F6", "routes", "03-f6-routes"), ("F3", "inventory", "04-f3-inventory"),
                     ("F4", "hangar", "05-f4-hangar"), ("F12", "settings", "06-f12-settings")]

            # ---- P4: feste Breite in allen Ansichten ---------------------------
            widths = {}
            for fk, mode, shotname in modes:
                await key(fk)
                await asyncio.sleep(0.9)
                m = await shellMetrics()
                widths[mode] = m
                check(f"p4-width-{mode}", m and abs(m["w"] - expectedW) <= 2, f"got {m}, expected {expectedW}")
                # K2: Shell horizontal zentriert (margin:0 auto auf .shell).
                expLeft = round((VW - m["w"]) / 2)
                check(f"p4-centered-{mode}", m and abs(m["left"] - expLeft) <= 2, f"left={m['left']}, expected {expLeft}")
                ovf = await js(f"document.scrollingElement.scrollWidth <= {VW} ? 'ok' : 'OVERFLOW'")
                check(f"p4-no-hoverflow-{mode}", ovf == "ok", str(ovf))
                await shot(shotname + ".png")

            same = len(set((m or {}).get("w") for m in widths.values())) == 1
            check("p4-width-identical-all-views", same, json.dumps(widths))

            # ---- P3: lange Liste scrollt innen ---------------------------------
            await key("F1"); await asyncio.sleep(0.6)
            await typeIntoSearch("gold")
            await asyncio.sleep(0.8)
            listM = await js("""(() => {
                const l = document.querySelector('.list');
                if (!l) return null;
                const s = document.querySelector('.shell').getBoundingClientRect();
                return { sh: l.scrollHeight, ch: l.clientHeight, shellBottom: Math.round(s.bottom) };
            })()""")
            check("p3-list-scrolls-inner", listM and listM["sh"] > listM["ch"], json.dumps(listM))
            check("p3-shell-in-viewport-search", listM and listM["shellBottom"] <= int(0.92 * VH), json.dumps(listM))
            await shot("07-search-long-list.png")

            # scrollTop bewegbar?
            sc = await js("""(() => {
                const l = document.querySelector('.list');
                l.scrollTop = 200;
                return l.scrollTop;
            })()""")
            check("p3-list-scrolltop-moves", sc == 200, str(sc))
            await shot("08-search-long-list-scrolled.png")

            # ---- P3: Hangar-Liste scrollt innen ---------------------------------
            await key("F4"); await asyncio.sleep(1.2)
            hangM = await js("""(() => {
                const l = document.querySelector('.hangar-list');
                if (!l) return null;
                const s = document.querySelector('.shell').getBoundingClientRect();
                return { sh: l.scrollHeight, ch: l.clientHeight, shellBottom: Math.round(s.bottom) };
            })()""")
            check("p3-hangar-list-scrolls-inner", hangM and hangM["sh"] > hangM["ch"], json.dumps(hangM))
            check("p3-shell-in-viewport-hangar", hangM and hangM["shellBottom"] <= int(0.92 * VH), json.dumps(hangM))
            await shot("09-hangar-30-ships.png")

            # ---- P12: Skalierung -------------------------------------------------
            await key("F12"); await asyncio.sleep(0.8)
            chips = await js("[...document.querySelectorAll('.cfg-sect .chip')].map(b => b.innerText.trim()).filter(t => /%/.test(t))")
            check("p12-scale-chips-present", chips and chips == ["80 %", "90 %", "100 %", "110 %", "125 %"], json.dumps(chips))
            await shot("10-settings-scale-chips.png")

            async def clickScaleChip(pct):
                await js(f"""(() => {{
                    const b = [...document.querySelectorAll('.cfg-sect .chip')]
                        .find(b => b.innerText.trim() === '{pct} %');
                    if (!b) return 'MISSING';
                    b.click();
                    return 'CLICKED';
                }})()""")
                await asyncio.sleep(0.5)

            async def scaleState():
                return await js("""(() => {
                    const v = document.documentElement.style.getPropertyValue('--ui-scale').trim();
                    const s = document.querySelector('.shell');
                    const probe = document.querySelector('.rail .rentry');
                    return {
                        uiScale: v,
                        shellW: s ? Math.round(s.getBoundingClientRect().width) : null,
                        probeH: probe ? Math.round(probe.getBoundingClientRect().height) : null
                    };
                })()""")

            # 125 %
            await clickScaleChip(125)
            st125 = await scaleState()
            check("p12-scale-125-ui-prop", st125 and st125["uiScale"] == "1.25", json.dumps(st125))
            stub125 = await js("globalThis.__lastSettings ? globalThis.__lastSettings.scale : null")
            check("p12-scale-125-ipc-fired", stub125 == 125, str(stub125))
            check("p12-scale-125-shell-width-const", st125 and abs(st125["shellW"] - expectedW) <= 3, str(st125))
            await shot("11-scale-125.png")

            # 80 %
            await clickScaleChip(80)
            st80 = await scaleState()
            check("p12-scale-80-ui-prop", st80 and st80["uiScale"] == "0.8", json.dumps(st80))
            stub80 = await js("globalThis.__lastSettings ? globalThis.__lastSettings.scale : null")
            check("p12-scale-80-ipc-fired", stub80 == 80, str(stub80))
            check("p12-scale-80-shell-width-const", st80 and abs(st80["shellW"] - expectedW) <= 3, str(st80))
            if st125 and st80 and st125.get("probeH") and st80.get("probeH"):
                check("p12-scale-visual-size-changes", st125["probeH"] > st80["probeH"], f"125={st125['probeH']} 80={st80['probeH']}")
            await shot("12-scale-80.png")

            # 100 % zurück
            await clickScaleChip(100)
            st100 = await scaleState()
            check("p12-scale-100-ui-prop", st100 and st100["uiScale"] == "1", json.dumps(st100))

            # ---- Regression: Enter-Detail-Filter / Esc-Kette --------------------
            # Kanonisch wie enter-detail-filter-probe.py: input.q via
            # Prototype-Setter + InputEvent, Keys via document.dispatchEvent —
            # unabhängig vom DOM-Fokus.
            async def typeQuery(text):
                expr = (
                    "(() => {"
                    "var i=document.querySelector('input.q');"
                    "if (!i) return 'NO-INPUT';"
                    "var set=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;"
                    "set.call(i,'');"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:''}));"
                    "set.call(i," + json.dumps(text) + ");"
                    "i.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:" + json.dumps(text) + "}));"
                    "return i.value;"
                    "})()"
                )
                await js(expr)
                await asyncio.sleep(0.6)

            await key("F1"); await asyncio.sleep(0.6)
            await typeQuery("WARM")  # Warm-up (Stub-Debounce-Flush)
            await typeQuery("gold")
            await asyncio.sleep(0.4)
            await js("document.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}))")
            await asyncio.sleep(0.8)
            inDetail = await js("document.querySelector('.shell.in-detail') ? 'DETAIL' : 'NO-DETAIL'")
            check("reg-enter-detail-opens", inDetail == "DETAIL", str(inDetail))
            await shot("13-enter-detail.png")
            await js("document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))")
            await asyncio.sleep(0.5)
            back = await js("document.querySelector('.shell.in-detail') ? 'DETAIL' : 'NO-DETAIL'")
            check("reg-esc-back-from-detail", back == "NO-DETAIL", str(back))
            await js("document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))")
            await asyncio.sleep(0.5)
            q = await js("document.querySelector('input.q') ? document.querySelector('input.q').value : 'NO-INPUT'")
            check("reg-double-esc-clears-query", q == "", repr(q))

            # ---- Regression: Rail-Kollaps bei schmalem Screen --------------------
            await call("Emulation.setDeviceMetricsOverride",
                       {"width": 480, "height": 800, "deviceScaleFactor": 2, "mobile": False})
            await asyncio.sleep(0.8)
            comp = await js("document.querySelector('.rail.is-compact') ? 'COMPACT' : 'NOT-COMPACT'")
            check("reg-rail-compact-narrow", comp == "COMPACT", str(comp))
            await shot("14-rail-compact-480.png")
            await call("Emulation.setDeviceMetricsOverride",
                       {"width": VW, "height": VH, "deviceScaleFactor": 2, "mobile": False})
            await asyncio.sleep(0.8)
            comp2 = await js("document.querySelector('.rail.is-compact') ? 'COMPACT' : 'NOT-COMPACT'")
            check("reg-rail-full-wide", comp2 == "NOT-COMPACT", str(comp2))

    finally:
        server.terminate()
        proc.terminate()

    print(f"\n=== {len(PASS)} PASS / {len(FAIL)} FAIL ===")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

asyncio.run(main())
