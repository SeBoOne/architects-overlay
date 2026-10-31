#!/usr/bin/env python3
"""UG (t_a1f0d83f) Info-Popup-Politur probe — self-contained: static server +
headless Chromium + CDP.

Ersetzt die Erwartungen des alten info-popup-probe.py (t_f178b084, bleibt als
historischer Beleg) für den neuen Stand:

  1. „ⓘ“-Chip rechts neben dem Fokus/Klick-Button (unverändert aus TW).
  2. Klick öffnet das Info-Popup (advert-scrim, z-index 240; Shell wächst mit
     class="shell-info").
  3. Neuer Inhalt: Sebos Vector-Logo (info-logo) prominent MITTIG (~112 px,
     zentriert), Titel + Version v0.5.20 + Build-Datum, Abschnitte Über →
     Datenquellen → Rechtliches → Footer-Links, breites Popup (≥ 80 % der
     Shell), UEX-Badge + SC-Wiki-Badge mit Label „Schiffsdaten“ (KEIN
     „Modul-Kompatibilität“ mehr), CIG-„Made by the Community“-Badge
     (unverändertes Offiziel-Asset) + Quellenangabe, MIT-Lizenz-Hinweis.
  4. GEOMETRIE: .info-modal scrollHeight ≤ clientHeight (kein internes Scrollen)
     und Popup vollständig innerhalb der Shell.
  5. Link-Klicks rufen open_info_link mit Whitelist-kinds auf (uex_api,
     releases); Esc/✕ schließen, dahinter unangetastet.

Backend = Tauri-Stub (docs/scripts/smoke-stub.js), injected per-CDP.
"""
import asyncio, base64, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/info-popup-polish")))
OUT.mkdir(parents=True, exist_ok=True)
PORT = os.environ.get("INFO_PROBE_PORT", "8796")
CDP_PORT = "9234"
URL = f"http://127.0.0.1:{PORT}/"

PASS = 0
FAIL = 0


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS {name}")
    else:
        FAIL += 1
        print(f"  FAIL {name}" + (f"  {detail}" if detail else ""))


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", PORT, "--bind", "127.0.0.1",
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
    print("static server up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1920,1080", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-info-polish-probe",
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
        server.terminate(); proc.terminate()
        raise RuntimeError("chromium CDP page target did not come up")

    async with websockets.connect(ws, max_size=None) as cdp:
        mid = {"n": 0}
        pending = {}

        async def call(method, params=None):
            mid["n"] += 1
            cur = mid["n"]
            fut = asyncio.get_event_loop().create_future()
            pending[cur] = fut
            await cdp.send(json.dumps({"id": cur, "method": method, "params": params or {}}))
            return await fut

        asyncio.create_task(_recv_loop(cdp, pending))

        def ok(resp):
            if "error" in resp:
                raise RuntimeError(f"CDP error: {resp['error']}")
            return resp.get("result", {})

        async def js(expr):
            r = ok(await call("Runtime.evaluate",
                              {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                ed = r["exceptionDetails"]
                txt = ed.get("text", "") + " " + (ed.get("exception", {}).get("description", "") or "")
                print(f"  RUNTIME-EXC: {str(txt)[:300]}")
                return None
            return r.get("result", {}).get("value")

        async def wait_for(expr, timeout=10):
            end = asyncio.get_event_loop().time() + timeout
            v = None
            while asyncio.get_event_loop().time() < end:
                v = await js(expr)
                if v:
                    return v
                await asyncio.sleep(0.15)
            return v

        async def shot(name):
            r = ok(await call("Page.captureScreenshot", {"format": "png"}))
            data = base64.b64decode(r["data"])
            (OUT / f"{name}.png").write_bytes(data)
            print(f"  shot: {OUT / (name + '.png')}  ({len(data)} bytes)")

        async def key(k):
            await js("window.dispatchEvent(new KeyboardEvent('keydown',%s))"
                     % json.dumps({"key": k, "bubbles": True, "composed": True}))
            await asyncio.sleep(0.3)

        async def txt(sel):
            return await js(
                "(function(){var e=document.querySelector('" + sel + "');return e?e.textContent.trim():''})()")

        await call("Page.enable")
        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride",
                   {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await wait_for("document.querySelector('.spot-status .info-btn')")

        # ---- Gate 1: Chip RECHTS neben dem Fokus/Klick-Button ----------------
        chips = await js("""
            (function(){
              var st = document.querySelector('.spot-status');
              if (!st) return null;
              var btns = Array.from(st.querySelectorAll('button'));
              var info = st.querySelector('.info-btn');
              var idx = btns.indexOf(info);
              return { n: btns.length, idx: idx, labels: btns.map(b => b.textContent.trim()) };
            })()
        """)
        check("spot-status hat ⓘ-Chip", chips is not None and chips.get("idx", -1) > 0,
              str(chips))
        check("ⓘ-Chip direkt RECHTS vom Fokus/Klick-Button",
              chips is not None and chips.get("labels")[chips["idx"] - 1] in ("FOKUS", "KLICK"),
              str(chips))

        await shot("01-info-button")

        # ---- Gate 2: Klick öffnet das Popup; Shell wächst mit (shell-info) ---
        await js("document.querySelector('.spot-status .info-btn').click()")
        modal_open = await wait_for("!!document.querySelector('.info-modal')")
        check("Klick auf ⓘ öffnet das Info-Popup", bool(modal_open))
        zi = await js("""
            (function(){
              var s = document.querySelector('.info-modal').closest('.advert-scrim');
              return s ? getComputedStyle(s).zIndex : null;
            })()
        """)
        check("Scrim liegt ÜBER den übrigen Popups (z-index 240)", zi == "240", str(zi))
        si = await js("""
            (function(){
              var sh = document.querySelector('.shell');
              return { cls: sh.classList.contains('shell-info'),
                       mh: getComputedStyle(sh).minHeight,
                       h: sh.getBoundingClientRect().height };
            })()
        """)
        check("Shell wächst mit (shell-info + min-height-Floor)",
              bool(si) and si.get("cls") and si.get("mh") not in (None, "", "0px"), str(si))

        # ---- Gate 3: Neuer Inhalt --------------------------------------------
        logo = await js("""
            (function(){
              var m = document.querySelector('.info-modal');
              var i = m ? m.querySelector('.info-logo') : null;
              if (!i) return null;
              var r = i.getBoundingClientRect();
              var mr = m.getBoundingClientRect();
              return { tag: i.tagName, src: i.getAttribute('src'), h: r.height,
                       centerOff: Math.abs((r.left + r.width / 2) - (mr.left + mr.width / 2)),
                       headChild: !!i.closest('.info-head') };
            })()
        """)
        check("Sebos Vector-Logo (info-logo IMG)", bool(logo) and logo.get("tag") == "IMG"
              and "/logo." in str(logo.get("src")), str(logo))
        check("Logo PROMINENT hoch (96–128 px)", bool(logo) and 96 <= logo.get("h", 0) <= 132,
              str(logo))
        check("Logo MITTIG zentriert (Abstand < 8 px)", bool(logo) and logo.get("centerOff", 999) < 8,
              str(logo))
        check("Logo oben im Kopf", bool(logo) and logo.get("headChild"), str(logo))
        title = await txt(".info-modal .info-title")
        check("Titel „Architects Overlay“", "Architects Overlay" in (title or ""), repr(title))
        meta = await txt(".info-modal .info-meta")
        check("Version v0.5.20 (tauri.conf.json)", "Version v0.5.20" in meta, meta)
        check("Build-Datum (YYYY-MM-DD)", "Build 20" in meta, meta)

        secs = await js("""
            (function(){
              var ts = Array.from(document.querySelectorAll('.info-modal .info-sec-title'))
                .map(e => e.textContent.trim());
              var f = !!document.querySelector('.info-modal .info-foot .info-links');
              return { secs: ts, footer: f };
            })()
        """)
        check("Abschnitte: Über → Datenquellen → Rechtliches (in dieser Reihenfolge)",
              bool(secs) and secs.get("secs") == ["Über", "Datenquellen", "Rechtliches"], str(secs))
        check("Footer mit Repo-/Release-Links", bool(secs) and secs.get("footer"), str(secs))
        about = await txt(".info-modal .info-sec .info-text")
        check("Über: Community-Projekt + Non-Commercial",
              bool(about) and "Community-Projekt" in about and "Non-Commercial" in about, about[:120])

        wide = await js("""
            (function(){
              var m = document.querySelector('.info-modal');
              var sh = document.querySelector('.shell');
              return { mw: m.getBoundingClientRect().width, sw: sh.getBoundingClientRect().width,
                       frac: m.getBoundingClientRect().width / sh.getBoundingClientRect().width };
            })()
        """)
        check("Popup deutlich breiter (≥ 80 % der Shell)", bool(wide) and wide.get("frac", 0) >= 0.80,
              str(wide))

        uex = await js("""
            (function(){
              var b = document.querySelector('.info-badge img[src*="uex-api-badge-powered"]');
              var t = b ? b.closest('.info-badge').textContent : '';
              return { img: !!b, hasApi: t.includes('api.uexcorp.space'),
                       label: t.includes('Daten') && t.includes('Marktplatz') };
            })()
        """)
        check("UEX-Badge: offizielles PNG + api.uexcorp.space + Label",
              bool(uex) and uex.get("img") and uex.get("hasApi") and uex.get("label"), str(uex))
        scw = await js("""
            (function(){
              var m = document.querySelector('.info-badge-mark');
              var b = m ? m.closest('.info-badge') : null;
              var all = Array.from(document.querySelectorAll('.info-modal')).map(x=>x.textContent).join('');
              return { mark: m ? m.textContent.trim() : null,
                       hasWiki: b ? b.textContent.includes('api.star-citizen.wiki') : false,
                       label: b ? b.textContent.includes('Schiffsdaten') : false,
                       labelAlt: b ? b.getAttribute('title') : null,
                       oldLabel: b ? b.textContent.includes('Modul-Kompatibilität') : false,
                       anywhereOld: all.includes('Modul-Kompatibilität') };
            })()
        """)
        check("SC-Wiki-Badge: SC-WIKI API + api.star-citizen.wiki + Label „Schiffsdaten“",
              bool(scw) and scw.get("mark") == "SC-WIKI API" and scw.get("hasWiki")
              and scw.get("label") and not scw.get("oldLabel"), str(scw))
        check("Altes Label „Modul-Kompatibilität“ nirgends mehr",
              bool(scw) and not scw.get("anywhereOld"), str(scw))
        check("SC-Wiki-Tooltip: „Schiffsdaten“", bool(scw) and "Schiffsdaten" in str(scw.get("labelAlt")), str(scw))

        cig = await js("""
            (function(){
              var img = document.querySelector('.info-cig img[src*="cig-made-by-the-community"]');
              var wrap = document.querySelector('.info-cig');
              return { img: !!img, alt: img ? img.getAttribute('alt') : null,
                       src: img ? img.getAttribute('src') : null,
                       srcNote: wrap ? wrap.textContent.includes('unverändert') : false,
                       nat: img ? (img.naturalWidth + 'x' + img.naturalHeight) : null };
            })()
        """)
        check("CIG-Badge „Made by the Community“ sichtbar (offizielles Asset)",
              bool(cig) and cig.get("img") and cig.get("alt") == "Made by the Community"
              and cig.get("nat") == "1418x1418", str(cig))
        check("CIG-Quellenangabe (unverändert/Fan-Asset)",
              bool(cig) and cig.get("srcNote"), str(cig))
        lic = await txt(".info-modal .info-license")
        check("Lizenz: MIT + LICENSE-Datei", bool(lic) and "MIT" in lic and "LICENSE" in lic, repr(lic))
        dis = await txt(".info-modal .info-disclaimer")
        check("Disclaimer: keine Verbindung zu CIG/RSI",
              "keiner Verbindung zu Cloud Imperium Games" in dis, dis[:120])
        check("Disclaimer: Markenhinweis (CIG Corp)",
              "Marken von Cloud Imperium Games Corp" in dis, dis[:120])

        # ---- Gate 4: GEOMETRIE — Popup ohne internen Scroll in der Shell -----
        geo = await js("""
            (function(){
              var m = document.querySelector('.info-modal');
              var sh = document.querySelector('.shell');
              var mr = m.getBoundingClientRect(), sr = sh.getBoundingClientRect();
              return { scrollH: m.scrollHeight, clientH: m.clientHeight,
                       fitsShell: mr.top >= sr.top - 1 && mr.bottom <= sr.bottom + 1 };
            })()
        """)
        check("Popup ohne internen Scroll (scrollH ≤ clientH)",
              bool(geo) and geo.get("scrollH", 1e9) <= geo.get("clientH", 0), str(geo))
        check("Popup vollständig innerhalb der Shell", bool(geo) and geo.get("fitsShell"), str(geo))

        await shot("02-info-popup")

        # ---- Gate 5: Links → open_info_link (Whitelist-kinds) ----------------
        await js("document.querySelectorAll('.info-badge')[0].click()")
        await asyncio.sleep(0.3)
        opens = await js("(window.__LAST_OPEN_INFO__ || []).join(',')")
        check("UEX-Badge-Klick → open_info_link(uex_api)", opens == "uex_api", str(opens))
        rel = await js("""
            (function(){
              var c = Array.from(document.querySelectorAll('.info-links .chip'))
                .find(e => e.textContent.includes('Releases'));
              if (c) c.click();
              return true;
            })()
        """)
        await asyncio.sleep(0.3)
        opens = await js("(window.__LAST_OPEN_INFO__ || []).join(',')")
        check("Releases-Chip (Footer) → open_info_link(releases)", opens == "uex_api,releases", str(opens))

        await shot("03-links-clicked")

        # ---- Gate 6: Esc schließt, dahinter unangetastet ---------------------
        await key("Escape")
        modal_gone = await wait_for("!document.querySelector('.info-modal')")
        check("Esc schließt das Info-Popup", bool(modal_gone))
        search_still = await js("!!document.querySelector('.spot-status')")
        check("UI dahinter unangetastet (spot-status noch da)", bool(search_still))
        await shot("04-after-esc")

        # ---- Gate 7: ✕ schließt (erneutes Öffnen) -----------------------------
        await js("document.querySelector('.spot-status .info-btn').click()")
        await wait_for("!!document.querySelector('.info-modal')")
        await js("document.querySelector('.info-modal .hd-close').click()")
        modal_gone2 = await wait_for("!document.querySelector('.info-modal')")
        check("✕-Button schließt das Info-Popup", bool(modal_gone2))

    proc.terminate()
    server.terminate()
    print(f"\nRESULT: {PASS} PASS, {FAIL} FAIL  →  shots in {OUT}")
    return 0 if FAIL == 0 else 1


async def _recv_loop(cdp, pending):
    async for raw in cdp:
        msg = json.loads(raw)
        if "id" in msg and msg["id"] in pending:
            pending[msg["id"]].set_result(msg)


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
