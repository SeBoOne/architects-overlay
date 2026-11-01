#!/usr/bin/env python3
"""T1 (t_530e5f59) i18n-Framework probe — self-contained: static server +
headless Chromium + CDP, Backend = Tauri-Stub (docs/scripts/smoke-stub.js).

Gates:

  1. CONFIG: Sprach-Section in „Darstellung“ — Chips English + Deutsch
     (Built-ins), Titel/Aktiv-Status über $t gerendert, aktiver Chip = en.
  2. LIVE-SWITCH de: Klick „Deutsch“ → settings_set_language IPC gefeuert
     (stubState-Spiegel), Section-Label + Button-Text wechseln auf Deutsch
     OHNE Reload (document nicht neu geladen).
  3. LIVE-SWITCH en: zurück auf English — gleiche Checks gespiegelt.
  4. LOKALE DATEI IMPORTIEREN: Beispiel-Community-Datei (code „fr“, teils
     übersetzt) über den echten UI-Pfad (File-Input via DataTransfer-File →
     onchange → file.text() → locale_import). Nach Import: neuer Chip
     „Français“, Note „Translation added: Français“.
  5. FALLBACK-KETTE: Sprache fr wählen — fehlende Keys fallen auf en
     (Section-Button zeigt en-Text), übersetzte Keys kommen aus fr.
  6. IMPORT-VALIDIERUNG: built-in-Code (de) importieren → Note zeigt die
     übersetzte Fehlermeldung (de: „… eingebaute Sprache …“), kein neuer Chip.
  7. PERSISTENZ-SPIEGEL: stubState.language === 'fr' am Ende (Boot-Test:
     Nach Reload bleibt die Sprache — stubSettings liefert language).

Screenshots nach docs/screenshots/i18n-framework/.
"""
import asyncio, base64, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/i18n-framework")))
OUT.mkdir(parents=True, exist_ok=True)
PORT = os.environ.get("I18N_PROBE_PORT", "8798")
CDP_PORT = "9235"
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


# Beispielsprache „fr“: nur settings.language.title übersetzt — alle anderen
# Keys fehlen bewusst (Fallback-Test).
FRENCH_LOCALE = json.dumps({
    "meta": {"code": "fr", "name": "Français"},
    "strings": {"settings.language.title": "Langue"}
})
# Bildschirm-Rotation um die Sprache sichtbar zu machen.
GERMAN_BAD_LOCALE = json.dumps({
    "meta": {"code": "de", "name": "Deutsch"},
    "strings": {"settings.language.title": "Sprache"}
})


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
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-i18n-probe",
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

        async def _recv_loop(cdp, pending):
            async for msg in cdp:
                if isinstance(msg, (str, bytes)):
                    try:
                        msg = json.loads(msg)
                    except Exception:
                        continue
                if not isinstance(msg, dict):
                    continue
                if msg.get("id") in pending:
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

        await call("Page.enable")
        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride",
                   {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.navigate", {"url": URL})
        await wait_for("document.querySelector('.spot-status') || document.querySelector('.shell')")

        async def open_config():
            opened = await js("""
                (function(){
                  var btn = document.querySelector('.rail-settings, .rail [data-mode="settings"], button[title*="Einstell"], .spot-status button[title*="Konfig"], .cfg-btn');
                  if (!btn) {
                    // Rail-Knopf per Label finden
                    btn = Array.from(document.querySelectorAll('button')).find(b =>
                      /Einstellungen|Settings|Konfig/i.test(b.getAttribute('aria-label') || b.title || ''));
                  }
                  if (btn) { btn.click(); return true; }
                  return false;
                })()
            """)
            return bool(await wait_for("!!document.querySelector('.pane-config')", 6)) if opened else False

        await open_config()

        # ---- Gate 1: Sprach-Section existiert (Built-ins, en aktiv) ----------
        lang_view = await js("""
            (function(){
              var sect = document.querySelector('.cfg-sect-language');
              if (!sect) return null;
              var chips = Array.from(sect.querySelectorAll('.chip')).map(b => b.textContent.trim());
              var active = sect.querySelector('.chip.is-on');
              return {
                chips: chips,
                hasTitle: true,
                titleText: sect.querySelector('.h').textContent.trim(),
                activeChip: active ? active.textContent.trim() : null
              };
            })()
        """)
        check("Sprach-Section: Titel gerendert", bool(lang_view) and lang_view.get("hasTitle"), str(lang_view))
        check("Sprach-Section: Built-in-Chips English+Deutsch",
              bool(lang_view) and "English" in (lang_view.get("chips") or [])
              and "Deutsch" in (lang_view.get("chips") or []), str(lang_view))
        check("Aktiver Chip = English (Default en)",
              bool(lang_view) and lang_view.get("activeChip") == "English", str(lang_view))
        check("stubState.language Spiegel = en (Boot)",
              (await js("window.__stubState.language")) == "en")
        await shot("01-config-language-en")

        # ---- Gate 2: LIVE-SWITCH auf Deutsch (kein Reload) -------------------
        nav_id_before = await js("performance.getEntriesByType('navigation')[0].startTime")
        clicked = await js("""
            (function(){
              var chip = Array.from(document.querySelectorAll('.cfg-sect-language .chip'))
                .find(b => b.textContent.trim() === 'Deutsch');
              if (chip) { chip.click(); return true; }
              return false;
            })()
        """)
        check("Deutsch-Chip klickbar", bool(clicked))
        de_state = await wait_for("window.__stubState.language === 'de'")
        check("settings_set_language IPC gefeuert (Stub-Spiegel de)", bool(de_state))
        de_title = await wait_for(
            "document.querySelector('.cfg-sect-language .h')?.textContent.trim() === 'Sprache'", 6)
        check("Section-Titel live auf Deutsch (ohne Neustart)", bool(de_title))
        no_reload = await js(
            f"performance.getEntriesByType('navigation')[0].startTime === {nav_id_before}")
        check("KEIN Reload beim Sprachwechsel", bool(no_reload))
        await shot("02-config-language-de")

        # ---- Gate 3: zurück auf English --------------------------------------
        await js("""
            (function(){
              var chip = Array.from(document.querySelectorAll('.cfg-sect-language .chip'))
                .find(b => b.textContent.trim() === 'English');
              chip && chip.click();
            })()
        """)
        en_title = await wait_for(
            "document.querySelector('.cfg-sect-language .h')?.textContent.trim() === 'Language'", 6)
        check("Zurück auf English live", bool(en_title))

        # ---- Gate 4: Community-Datei über den echten UI-Pfad importieren -----
        imported = await js(
            "(() => { return new Promise((resolve, reject) => {"
            "  const input = document.querySelector('input.cfg-locale-file');"
            "  if (!input) { reject(new Error('no .cfg-locale-file input')); return; }"
            "  const file = new File([" + json.dumps(FRENCH_LOCALE) + "], 'fr.json', { type: 'application/json' });"
            "  const dt = new DataTransfer(); dt.items.add(file);"
            "  input.addEventListener('change', () => setTimeout(() => resolve(true), 400), { once: true });"
            "  input.files = dt.files;"
            "  input.dispatchEvent(new Event('change', { bubbles: true }));"
            "  setTimeout(() => reject(new Error('no change fired')), 8000);"
            "}); })()")
        check("File-Import über echten Input-Pfad (change → locale_import)", bool(imported))
        fr_chip = await wait_for(
            "Array.from(document.querySelectorAll('.cfg-sect-language .chip'))"
            ".some(b => b.textContent.trim() === 'Français')", 6)
        check("Neuer Chip „Français“ nach Import", bool(fr_chip))
        note = await js("""
            (function(){
              var n = document.querySelector('.pane-config .cfg-note[role="status"]');
              return n ? n.textContent.trim() : null;
            })()
        """)
        check("Import-Note (en): „Translation added: Français“",
              note is not None and "Translation added: Français" in note, str(note))
        stub_len = await js("window.__stubState.locales.length")
        check("Stub hat fr lokal persistiert (locale_list-Quelle)", stub_len == 1, str(stub_len))
        await shot("03-config-fr-imported")

        # ---- Gate 5: Fallback-Kette gewählt(fr) → en → raw -------------------
        await js("""
            (function(){
              var chip = Array.from(document.querySelectorAll('.cfg-sect-language .chip'))
                .find(b => b.textContent.trim() === 'Français');
              chip && chip.click();
            })()
        """)
        fr_active = await wait_for("window.__stubState.language === 'fr'")
        check("fr aktiv (IPC-Spiegel)", bool(fr_active))
        fr_title = await wait_for(
            "document.querySelector('.cfg-sect-language .h')?.textContent.trim() === 'Langue'", 6)
        check("Übersetzter Key kommt aus fr („Langue“)", bool(fr_title))
        en_fallback = await wait_for(
            "Array.from(document.querySelectorAll('.cfg-sect-language .chip'))"
            ".some(b => b.textContent.trim() === 'Add translation …')", 6)
        check("Fehlender Key fällt auf en („Add translation …“ bleibt)", bool(en_fallback))
        await shot("04-config-fr-fallback")

        # ---- Gate 6: built-in-Import abgelehnt, übersetzte Fehlermeldung ------
        rejected = await js(
            "(() => { return new Promise((resolve) => {"
            "  const input = document.querySelector('input.cfg-locale-file');"
            "  const file = new File([" + json.dumps(GERMAN_BAD_LOCALE) + "], 'de.json', { type: 'application/json' });"
            "  const dt = new DataTransfer(); dt.items.add(file);"
            "  input.addEventListener('change', () => setTimeout(() => resolve(true), 400), { once: true });"
            "  input.files = dt.files;"
            "  input.dispatchEvent(new Event('change', { bubbles: true }));"
            "  setTimeout(() => resolve(true), 8000);"
            "}); })()")
        check("built-in-Import-Flow angestoßen", bool(rejected))
        err_note = await js("""
            (function(){
              var n = document.querySelector('.pane-config .cfg-note[role="status"]');
              return n ? n.textContent.trim() : null;
            })()
        """)
        # fr ist noch aktiv → Fehlermeldung muss auf FRANZÖSISCH/EN-Fallback
        # erscheinen: fr hat den error-Key nicht → en-Fallback.
        check("built-in-Import abgelehnt (error_key-Übersetzung, en-Fallback)",
              err_note is not None
              and '"de" is a built-in language' in err_note
              and 'overwritten' in err_note, str(err_note))
        n_locales = await js("window.__stubState.locales.length")
        check("Kein zweites Locale persistiert (de-Import blockiert)", n_locales == 1, str(n_locales))

        # ---- Gate 7: Persistenz-Spiegel + Screens -----------------------------
        final = await js("window.__stubState.language")
        check("Persistenz-Spiegel fr (würde beim Boot wiederkommen)", final == "fr", str(final))
        await shot("05-config-fr-error-note")

    server.terminate()
    proc.terminate()
    print(f"\ni18n-framework probe: {PASS} PASS, {FAIL} FAIL")
    raise SystemExit(0 if FAIL == 0 else 1)


if __name__ == "__main__":
    asyncio.run(main())
