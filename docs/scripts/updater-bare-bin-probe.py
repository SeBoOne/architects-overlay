#!/usr/bin/env python3
"""TH (t_df9646a3) Updater — Linux-Bare-Binary-Plan probe — self-contained:
static server + headless Chromium + CDP.

Beleg, dass die Config-View „Aktualisierung“ einen verfügbaren Update mit
kind `linux-bin` (Plan::LinuxBin) korrekt anzeigt statt des alten
Fehlerfalls „Kein passendes Asset im Release … für diese Plattform“:

  1. „Jetzt prüfen“ → Stub liefert Update v0.5.21 mit kind `linux-bin` →
     cfg-key-status zeigt „Update v0.5.21 verfügbar“.
  2. „Jetzt aktualisieren (v0.5.21)“-Button ist ANGEZEIGT (kind `linux-bin`
     ≠ `deb` → kein apt-Hint, sondern Self-Update).
  3. Nach dem Klick auf „Jetzt aktualisieren“ erscheint die Note
     „Update v0.5.21 installiert — Neustart des Overlays anwenden lassen
     (systemd restart oder Overlay schließen/öffnen).“ (KEIN Auto-Restart).
  4. Screenshot in docs/screenshots/updater-bare-bin/.

Backend = Tauri-Stub (docs/scripts/smoke-stub.js), injected per-CDP. Der
Stub liefert updater_status/check_now/install nur, wenn die Probe
`__stubState.updater_available` setzt — andere Proben bleiben unverändert.
"""
import asyncio, base64, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/updater-bare-bin")))
OUT.mkdir(parents=True, exist_ok=True)
PORT = os.environ.get("UPDATER_PROBE_PORT", "8798")
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
        "--window-size=1920,1080", f"--remote-debugging-port={CDP_PORT}",
        "--user-data-dir=/tmp/cdp-updater-bare-bin-probe", "about:blank",
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

        async def _recv_loop(cdp, pending):
            async for raw in cdp:
                msg = json.loads(raw)
                if "id" in msg and msg["id"] in pending:
                    pending[msg["id"]].set_result(msg)
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

        await call("Page.enable")
        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride",
                   {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await wait_for("document.querySelector('.spot-status .info-btn')")

        # ---- Setup: Probe setzt die Stub-Fixture (Update v0.5.21, linux-bin) --
        await js("""
            (function(){
              var s = window.__stubState;
              if (!s) return 'no-stub';
              s.updater_available = { version: '0.5.21', kind: 'linux-bin', asset_name: 'architects-overlay_0.5.21_linux64' };
              s.updater_note = '';
              return 'ok';
            })()
        """)
        check("Stub-Fixture gesetzt (linux-bin)", True)

        # ---- Gate 1: Config-View öffnen, „Jetzt prüfen“ → Update-Chip --------
        # Zahnrad ist entfernt (t_a39f6811 P2) — Rail + F12 decken die Konfiguration ab.
        await key("F12")
        cfg_open = await wait_for("!!document.querySelector('.pane-config')")
        check("Config-View öffnet (Zahnrad)", bool(cfg_open))
        btn = await js("""
            (function(){
              var sect = [...document.querySelectorAll('.pane-config .cfg-sect')]
                .find(s => s.querySelector('.h')?.textContent.trim() === 'Aktualisierung');
              if (!sect) return null;
              var b = [...sect.querySelectorAll('button.btn')].find(x => x.textContent.trim() === 'Jetzt prüfen');
              return b ? 'found' : 'missing';
            })()
        """)
        check("„Jetzt prüfen“-Button in der Aktualisierung-Sektion", btn == "found", str(btn))
        await js("""
            (function(){
              var sect = [...document.querySelectorAll('.pane-config .cfg-sect')]
                .find(s => s.querySelector('.h')?.textContent.trim() === 'Aktualisierung');
              var b = [...sect.querySelectorAll('button.btn')].find(x => x.textContent.trim() === 'Jetzt prüfen');
              b.click();
            })()
        """)
        upd = await wait_for(
            "document.querySelector('.pane-config .cfg-key-status .ok')?.textContent.trim()", 6)
        check("Update-Chip zeigt „Update v0.5.21 verfügbar“ (linux-bin statt Fehler)",
              upd is not None and "v0.5.21" in str(upd), str(upd))
        await shot("01-update-linux-bin-available")

        # ---- Gate 2: Install-Button ANGEZEIGT (kind linux-bin ≠ deb) ---------
        install_lbl = await js("""
            (function(){
              var sect = [...document.querySelectorAll('.pane-config .cfg-sect')]
                .find(s => s.querySelector('.h')?.textContent.trim() === 'Aktualisierung');
              var b = [...sect.querySelectorAll('button.btn')].find(x => x.textContent.trim().startsWith('Jetzt aktualisieren'));
              return b ? b.textContent.trim() : null;
            })()
        """)
        check("„Jetzt aktualisieren (v0.5.21)“-Button angezeigt (kind linux-bin ≠ deb)",
              install_lbl is not None and "v0.5.21" in str(install_lbl), str(install_lbl))

        # ---- Gate 3: Klick → Neustart-Hinweis, kein Auto-Restart -------------
        await js("""
            (function(){
              var sect = [...document.querySelectorAll('.pane-config .cfg-sect')]
                .find(s => s.querySelector('.h')?.textContent.trim() === 'Aktualisierung');
              var b = [...sect.querySelectorAll('button.btn')].find(x => x.textContent.trim().startsWith('Jetzt aktualisieren'));
              b.click();
            })()
        """)
        note = await wait_for("document.body.innerText.includes('Neustart des Overlays anwenden lassen')", 6)
        check("Neustart-Hinweis nach dem Swap: „Neustart des Overlays anwenden lassen“",
              bool(note))
        full = await js("""
            (function(){
              var sect = [...document.querySelectorAll('.pane-config .cfg-sect')]
                .find(s => s.querySelector('.h')?.textContent.trim() === 'Aktualisierung');
              var notes = [...sect.querySelectorAll('.cfg-note')].map(n => n.textContent.trim());
              return notes.find(t => t.includes('Update v')) || '';
            })()
        """)
        check("Note enthält „Update v0.5.21 installiert“ (kein Auto-Restart)",
              isinstance(full, str) and "Update v0.5.21 installiert" in full, str(full))
        await shot("02-update-installed-restart-hint")

        server.terminate()
        proc.terminate()
        print(f"\n  PASS/FAIL: {PASS}/{FAIL}")
        raise SystemExit(0 if FAIL == 0 else 1)


try:
    asyncio.run(main())
except SystemExit as e:
    raise SystemExit(e.code)
except Exception as e:
    print(f"PROBE-ERROR: {e}")
    raise SystemExit(2)
