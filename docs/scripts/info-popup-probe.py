#!/usr/bin/env python3
"""TW (t_f178b084) Info-Popup smoke probe — self-contained: static server +
headless Chromium + CDP.

Gates:
  1. „ⓘ“-Chip existiert RECHTS neben dem Fokus/Klick-Button (spot-status).
  2. Klick öffnet das Info-Popup (advert-scrim, z-index 240 — über allem).
  3. Popup-Inhalt: Lockup, Version v0.5.20 (= tauri.conf.json), Build-Datum,
     Community-Disclaimer, UEX-Badge (offizielles PNG), SC-Wiki-Text-Badge.
  4. Link-Klicks rufen open_info_link mit den Whitelist-kinds auf (Stub
     trackt: uex_api, releases; Esc/✕ schließen, dahinter bleibt unangetastet).

Backend = Tauri-Stub (docs/scripts/smoke-stub.js), injected per-CDP.
"""
import asyncio, base64, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/info-popup")))
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
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-info-probe",
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

        # ---- Gate 2: Klick öffnet das Popup (advert-scrim, z-index 240) ------
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

        # ---- Gate 3: Inhalt ---------------------------------------------------
        lockup = await js("""
            (function(){
              var i = document.querySelector('.info-modal .info-lockup');
              return i ? (i.tagName + '|' + (i.getAttribute('src') || '')) : null;
            })()
        """)
        check("Lockup im Kopf", bool(lockup) and lockup.startswith("IMG") and len(lockup) > 10,
              str(lockup))
        meta = await txt(".info-modal .info-meta")
        check("Version v0.5.20 (tauri.conf.json)", "Version v0.5.20" in meta, meta)
        check("Build-Datum (YYYY-MM-DD)", "Build 20" in meta and len(meta.split("Build ")[-1][:10]) == 10,
              meta)
        dis = await txt(".info-modal .info-disclaimer")
        check("Community-Disclaimer: inoffizielles Community-Projekt",
              "inoffizielles Community-Projekt" in dis, dis[:120])
        check("Disclaimer: nicht mit CIG/RSI verbunden",
              "keiner Verbindung zu Cloud Imperium Games" in dis, dis[:120])
        check("Disclaimer: Markenhinweis (RSI-Logo, CIG Corp)",
              "Marken von Cloud Imperium Games Corp" in dis, dis[:120])
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
              var b = m ? m.closest('.info-badge').textContent : '';
              return { mark: m ? m.textContent.trim() : null,
                       hasWiki: b.includes('api.star-citizen.wiki'),
                       label: b.includes('Modul-Kompatibilität') };
            })()
        """)
        check("SC-Wiki-Text-Badge: SC-WIKI API + api.star-citizen.wiki + Label",
              bool(scw) and scw.get("mark") == "SC-WIKI API" and scw.get("hasWiki") and scw.get("label"),
              str(scw))
        links = await js("""
            (function(){
              var c = Array.from(document.querySelectorAll('.info-links .chip'))
                .map(e => e.textContent.trim());
              return c;
            })()
        """)
        check("Repo-/Release-Links vorhanden",
              bool(links) and any("GitHub-Repo" in c for c in links)
              and any("Releases" in c and "Changelog" in c for c in links), str(links))

        await shot("02-info-popup")

        # ---- Gate 4: Links → open_info_link (Whitelist-kinds) -----------------
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
        check("Releases-Chip → open_info_link(releases)", opens == "uex_api,releases", str(opens))

        # ---- Gate 5: Esc schließt, dahinter bleibt unangetastet ---------------
        before = await js("document.body.dataset ? true : true") and True
        await key("Escape")
        modal_gone = await wait_for("!document.querySelector('.info-modal')")
        check("Esc schließt das Info-Popup", bool(modal_gone))
        search_still = await js("!!document.querySelector('.spot-status')")
        check("UI dahinter unangetastet (spot-status noch da)", bool(search_still))
        await shot("03-after-esc")

        # ---- Gate 6: ✕ schließt (erneutes Öffnen) ------------------------------
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
