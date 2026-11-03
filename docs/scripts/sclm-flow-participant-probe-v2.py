#!/usr/bin/env python3
"""CDP-Probe für t_cce3a5ea (SCLM v2 Karte C): Teilnehmer-Flow v2 — UI-Ebene.

Ersetzt die SF2-Probe (sclm-flow-participant-probe.py, c554192): deren
code_entry-/participants-/addme-Screens existieren im v2-Manifest nicht mehr.
Datenquelle STUBS (smoke-stub.js + invoke-Override für die v2-Befehle
connector_flow_open/keystore_find/scan_match/screen_action); die echte
Svelte-UI läuft unverändert (pnpm build + statischer Server). Bootisolierte
Chromium-Profile (Lektion aus Karte-B-Review-Runde-1: wiederverwendete
Profile kontaminieren die Sonden).

Boote und Prüfketten (Sprache EN für stabile Textanker):
  A) Suchzeilen-Einstieg + Vollkette:
     E0 Suchmodus-Eintritt berührt flow_open NICHT (Modus-Gating Req 1)
     E1 own-Modus (F7): Suchleiste = Code-Feld (Platzhalter code_ph)
     F1 Enter → connector_flow_open mit getipptem Code
     F2 wishes-Screen direkt (kein code_entry)
     F3 KEINE ftabs-Schritt-Tabs bei open-Flow (Req 9)
     F4 pcode-Feld aus Flow-Zustand vorseediert (Keystore-Spiegel)
     F5 Confirm → connector_screen_action: loot_id-Array (2), pcode,
        participant_id, sid/code, raffle_mode — pcode nur als Body-Wert
     F6 danach wait (Poll-Hinweis)
     F7 'Change wishes' → zurück zu wishes (nav ohne HTTP)
     F8 Confirm #2 → automatisch zurück auf wait
     F9 Poll-Exit: raffle_done-Flip → automatisch results (17 s Poll-Fenster)
     F10 Gewinn-Detail + collect/waive AKTIV (Flags 0)
  B) done-Weiche + null-Ergebnis:
     D1 flow_open done_screen → direkt results (wishes nie gesehen)
     D2 result null → 'Result still open' + Poll-Hinweis (poll_seconds 60)
  C) one-way (Req 9 enabled_if):
     D3 collect+waive deaktiviert bei collected:1/waived:1
     D4 Disabled-Titel 'Already set'
     D5 Klick auf deaktivierte Aktion → kein connector_screen_action
  D) Öffnen-Button (Karte-B-Andockung) + Legacy-Tabs:
     G1 keystore_find mit {id, sid}   G2 flow_open mit gespeichertem Code
     G3 Status-Chips pending/raffled + Öffnen-Button aktiv
     H1 Creator-Flow (Legacy ohne open-Block) behält .ftabs-Navigation

Screenshots: docs/screenshots/sclm-flow-participant-v2/
Voraussetzung: pnpm build (build/ im Repo-Root), chromium, websockets.
Start: python3 docs/scripts/sclm-flow-participant-probe-v2.py
"""
import asyncio, base64, json, os, pathlib, shutil, sys, urllib.request
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
MANIFEST = json.loads((ROOT / "examples/sclm.json").read_text())
LOCALE = json.loads((ROOT / "resources/locales/en.json").read_text())["strings"]
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screenshots/sclm-flow-participant-v2")))
OUT.mkdir(parents=True, exist_ok=True)
HTTP_PORT = 8798
URL = f"http://127.0.0.1:{HTTP_PORT}/"
CHROME_PORT0 = 9360  # je Boot: CHROME_PORT0 + boot_nr

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

ENTRY = {"id": "sclm", "enabled": True, "auth_set": {}, "hotkey": "F7",
         "settings_set": {"sclm_username": True}, "manifest": MANIFEST}
PCODE = "PC1234567890abcd"

def flow_state(raffle_done=0, code="ABC123"):
    return {"sid": "s1", "name": "Xmas Raffle", "max_wishes": 3, "raffle_mode": "wish",
            "raffle_done": raffle_done, "locked": 0, "code": code,
            "participant_id": "p1", "pcode": PCODE}

DATA_A = {
    "participant:wishes": {"rows": [
        {"name": "Cutlass Black", "loot_id": "L1", "qty": 1, "type": "Ship", "grade": "T1", "size": "M"},
        {"name": "Caterpillar", "loot_id": "L2", "qty": 1, "type": "Ship", "grade": "T1", "size": "S"},
        {"name": "Merlin", "loot_id": "L3", "qty": 1, "type": "Helmet", "grade": "T2", "size": "S"}]},
    "participant:wait": {"mapped": {"raffle_done": 0, "name": "Xmas Raffle", "locked": 0}},
    "participant:results": {"mapped": {"rid": "r1", "won": "Cutlass Black",
                                       "waived": 0, "collected": 0, "item_name": "Cutlass Black"}},
}
DATA_NULL_RESULTS = {**DATA_A, "participant:results": {"mapped": {
    "rid": None, "won": None, "waived": None, "collected": None, "item_name": None}}}
DATA_ONEWAY = {**DATA_A, "participant:results": {"mapped": {
    "rid": "r1", "won": "Cutlass Black", "waived": 1, "collected": 1, "item_name": "Cutlass Black"}}}

def scan_outcome():
    import time
    return {"scan_id": "my_raffles", "username_set": True, "skipped": 0, "cached": False,
            "fetched_at_ms": int(time.time() * 1000),
            "items": [{"sid": "s1", "name": "Xmas Raffle", "raffle_done": 0, "has_saved_code": True},
                      {"sid": "s2", "name": "Old Raffle", "raffle_done": 1, "has_saved_code": True}]}

def boot_src(data, flow_screen="wishes", flow_raffle=0):
    return STUB + "\n" + f"""
localStorage.clear();
window.__STUB_CONNECTORS__ = [{json.dumps(ENTRY)}];
window.__STUB_CONNECTOR_DATA__ = {json.dumps(data)};
window.__RB_SCAN__ = {json.dumps(scan_outcome())};
window.__P_FLOWS__ = [];
window.__P_ACTIONS__ = [];
window.__P_KEYSTORE__ = [];
window.__P_FETCHES__ = [];
window.__P_FLOW_RESULT__ = {{flow_id: 'participant', screen: {json.dumps(flow_screen)},
  state: {json.dumps(flow_state(flow_raffle))}, joined: true, claimed: false,
  pcode_generated: false, status: 200}};
window.__stubState.language = 'en';
(function() {{
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {{
    if (cmd === 'connector_flow_open') {{ const st = JSON.parse(JSON.stringify(window.__P_FLOW_RESULT__)); if (args && args.code) st.state.code = args.code; window.__P_FLOWS__.push(args || {{}}); return st; }}
    if (cmd === 'connector_keystore_find') {{ window.__P_KEYSTORE__.push(args || {{}}); return {{join_code: 'ABC123', pcode: '{PCODE}'}}; }}
    if (cmd === 'connector_scan_match') return window.__RB_SCAN__;
    if (cmd === 'connector_get_settings_fields') return [];
    if (cmd === 'connector_screen_action') {{ window.__P_ACTIONS__.push(args || {{}}); return {{status: 200, body: {{ok: true}}}}; }}
    if (cmd === 'connector_fetch') window.__P_FETCHES__.push(args && args.viewId);
    return orig(cmd, args);
  }};
}})();
"""

class Boot:
    """Bootisoliertes Chromium (frisches Profil + eigener Debug-Port)."""
    def __init__(self, n):
        self.n = n
        self.udd = f"/tmp/v2cdp-{n}"
        self.port = CHROME_PORT0 + n
        self.ws_url = None
        self.ws = None
        self.proc = None
        self._id = 0
        self._pending = {}

    async def open(self, src):
        shutil.rmtree(self.udd, ignore_errors=True)
        self.proc = await asyncio.create_subprocess_exec(
            "/usr/bin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
            "--window-size=1920,1080", "--hide-scrollbars",
            f"--remote-debugging-port={self.port}", f"--user-data-dir={self.udd}",
            "about:blank", stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
        for _ in range(80):
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{self.port}/json/list", timeout=1) as r:
                    pages = [t for t in json.load(r) if t.get("type") == "page"]
                if pages:
                    self.ws_url = pages[0]["webSocketDebuggerUrl"]
                    break
            except Exception:
                pass
            await asyncio.sleep(0.25)
        if self.ws_url is None:
            raise RuntimeError(f"chromium CDP did not come up (boot {self.n})")
        self.ws = await websockets.connect(self.ws_url, max_size=None)
        async def recv_loop():
            async for raw in self.ws:
                msg = json.loads(raw)
                if "id" in msg and msg["id"] in self._pending:
                    self._pending[msg["id"]].set_result(msg)
        asyncio.create_task(recv_loop())
        await self.call("Page.enable"); await self.call("Runtime.enable")
        await self.call("Emulation.setDeviceMetricsOverride",
                        {"width": 1920, "height": 1080, "deviceScaleFactor": 2, "mobile": False})
        await self.call("Page.addScriptToEvaluateOnNewDocument", {"source": src})
        await self.call("Page.navigate", {"url": URL})
        await asyncio.sleep(2.2)

    async def call(self, method, params=None):
        self._id += 1
        fut = asyncio.get_event_loop().create_future()
        self._pending[self._id] = fut
        await self.ws.send(json.dumps({"id": self._id, "method": method, "params": params or {}}))
        resp = await fut
        if "error" in resp:
            raise RuntimeError(f"CDP error: {resp['error']}")
        return resp.get("result", {})

    async def js(self, expr):
        r = await self.call("Runtime.evaluate",
                            {"expression": expr, "returnByValue": True, "awaitPromise": True})
        if r.get("exceptionDetails"):
            raise RuntimeError("JS exception: " + json.dumps(r["exceptionDetails"])[:300])
        return (r.get("result") or {}).get("value")

    async def key(self, k, vk):
        await self.call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                          "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
        await self.call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                          "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})

    async def type_code(self, code):
        await self.js("var q=document.querySelector('.q'); q.focus();"
                      f"q.value={json.dumps(code)}; q.dispatchEvent(new Event('input',{{bubbles:true}}));")
        await self.key("Enter", 13)
        await asyncio.sleep(1.4)

    async def click_text(self, text, selector="button"):
        return await self.js(
            "(()=>{const b=[...document.querySelectorAll(%s)].find(x=>x.innerText.trim().toLowerCase()===%s);"
            "if(!b)return 'NOBTN'; b.click(); return 'OK';})()" % (json.dumps(selector), json.dumps(text.lower())))

    async def btn_state(self, text):
        return await self.js(
            "(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.innerText.trim().toLowerCase()===%s);"
            "return b?{found:true, disabled:b.disabled, title:b.title}:{found:false};})()" % json.dumps(text.lower()))

    async def dump(self, name):
        return await self.js(f"JSON.stringify(window.{name})")

    async def shot(self, name):
        r = await self.call("Page.captureScreenshot", {"format": "png"})
        (OUT / name).write_bytes(base64.b64decode(r["data"]))
        print("  shot:", OUT / name)

    async def close(self):
        try: await self.ws.close()
        except Exception: pass
        if self.proc:
            self.proc.terminate()
            try: await asyncio.wait_for(self.proc.wait(), 5)
            except Exception: self.proc.kill()


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(HTTP_PORT), "--bind", "127.0.0.1",
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
        server.terminate(); raise RuntimeError("static server down (pnpm build laufen?)")

    boot_nr = 0
    try:
        # ---------- Boot A: Suchzeilen-Einstieg + Vollkette ----------
        boot_nr += 1
        b = Boot(boot_nr); await b.open(boot_src(DATA_A))
        try:
            body = (await b.js("document.body.innerText") or "").lower()
            ph = await b.js("document.querySelector('.q')?.placeholder || 'NONE'")
            check("E0-code-field-only-in-own-mode", ph != LOCALE["connector.flow.code_ph"], f"ph={ph}")
            await b.type_code("AB12CD")
            flows = json.loads(await b.dump("__P_FLOWS__") or "[]")
            check("E0-search-mode-enter-no-flow-open", len(flows) == 0, str(flows))
            await b.key("F7", 118); await asyncio.sleep(1.2)
            ph = await b.js("document.querySelector('.q')?.placeholder || 'NONE'")
            check("E1-own-mode-code-placeholder", ph == LOCALE["connector.flow.code_ph"], f"ph={ph}")
            await b.shot("01-own-searchbar-code-entry.png")
            landing = await b.js("document.querySelector('.connector')?.innerText || ''")
            check("B-landing-scan-list-active", "xmas raffle" in landing.lower() and "old raffle" in landing.lower(), landing[:120])
            await b.type_code("AB12CD")
            flows = json.loads(await b.dump("__P_FLOWS__") or "[]")
            check("F1-flow-open-call", len(flows) == 1 and flows[0].get("id") == "sclm"
                  and flows[0].get("flow_id") == "participant" and flows[0].get("code") == "AB12CD", str(flows))
            body = (await b.js("document.body.innerText") or "").lower()
            check("F2-wishes-direct-no-code-entry", "mark wishes" in body and "loot_id" in body, body[:180])
            ftabs = await b.js("document.querySelectorAll('.ftabs').length")
            check("F3-no-step-tabs-open-flow", ftabs == 0, f"ftabs={ftabs}")
            pv = await b.js("document.querySelector('.factions input.finput')?.value ?? 'NONE'")
            check("F4-pcode-seeded-from-state", pv == PCODE, f"pcode={pv}")
            await b.js("[...document.querySelectorAll('.frow.fmark')].slice(0,2).forEach(x=>x.click())")
            await asyncio.sleep(0.4)
            marked_txt = (await b.js("document.querySelector('.fmarkinfo')?.innerText || ''") or "").lower()
            check("F5a-marked-counter", "2 of 3 marked" in marked_txt, marked_txt)
            await b.shot("02-wishes-marked.png")
            click = await b.click_text("Confirm wishes")
            check("F5b-confirm-clicked", click == "OK", str(click))
            await asyncio.sleep(1.0)
            acts = json.loads(await b.dump("__P_ACTIONS__") or "[]")
            vals = (acts[0].get("values") or {}) if acts else {}
            check("F5c-action-array-and-body-pcode", len(acts) == 1
                  and acts[0].get("screenId") == "participant:wishes" and acts[0].get("actionId") == "confirm"
                  and isinstance(vals.get("loot_id"), list) and len(vals.get("loot_id") or []) == 2
                  and vals.get("pcode") == PCODE and vals.get("participant_id") == "p1"
                  and vals.get("sid") == "s1" and vals.get("code") == "AB12CD"
                  and vals.get("raffle_mode") == "wish", json.dumps(vals)[:500])
            body = (await b.js("document.body.innerText") or "").lower()
            check("F6-wait-after-confirm", "awaiting raffle" in body
                  and LOCALE["connector.flow.wait_poll"].replace("{s}", "15").lower() in body, body[:180])
            await b.shot("03-wait.png")
            click = await b.click_text("Change wishes")
            check("F7a-edit-wishes-nav", click == "OK", str(click))
            await asyncio.sleep(0.9)
            body = (await b.js("document.body.innerText") or "").lower()
            acts2 = json.loads(await b.dump("__P_ACTIONS__") or "[]")
            check("F7b-back-to-wishes-no-http", "mark wishes" in body and len(acts2) == 1, body[:150])
            await b.js("[...document.querySelectorAll('.frow.fmark')].slice(0,1).forEach(x=>x.click())")
            await b.click_text("Confirm wishes"); await asyncio.sleep(1.0)
            body = (await b.js("document.body.innerText") or "").lower()
            check("F8-auto-back-to-wait", "awaiting raffle" in body, body[:150])
            await b.js("window.__STUB_CONNECTOR_DATA__['participant:wait'].mapped.raffle_done = 1;")
            await asyncio.sleep(17.5)  # wait poll_seconds=15 → ein Poll-Zyklus
            body = (await b.js("document.body.innerText") or "").lower()
            check("F9-wait-exit-to-results", "my win" in body and "awaiting raffle" not in body, body[:200])
            check("F10-win-detail", "cutlass black" in body, body[:200])
            st_c = await b.btn_state("Mark collected")
            st_w = await b.btn_state("Waive")
            check("F11-one-way-enabled-when-unset", st_c.get("found") and not st_c.get("disabled")
                  and st_w.get("found") and not st_w.get("disabled"), json.dumps([st_c, st_w]))
            await b.shot("04-results-open-actions.png")
        finally:
            await b.close()

        # ---------- Boot B: done-Weiche + result null ----------
        boot_nr += 1
        b = Boot(boot_nr); await b.open(boot_src(DATA_NULL_RESULTS, flow_screen="results", flow_raffle=1))
        try:
            await b.key("F7", 118); await asyncio.sleep(0.8)
            await b.type_code("ZZ99ZZ")
            body = (await b.js("document.body.innerText") or "").lower()
            check("D1-direct-results-on-done-open", "my win" in body and "mark wishes" not in body, body[:200])
            check("D2-result-null-pending", LOCALE["connector.flow.result_open"].lower() in body
                  and LOCALE["connector.flow.wait_poll"].replace("{s}", "60").lower() in body, body[:220])
            await b.shot("05-results-still-open.png")
        finally:
            await b.close()

        # ---------- Boot C: one-way deaktiviert (enabled_if) ----------
        boot_nr += 1
        b = Boot(boot_nr); await b.open(boot_src(DATA_ONEWAY, flow_screen="results", flow_raffle=1))
        try:
            await b.key("F7", 118); await asyncio.sleep(0.8)
            await b.type_code("ZZ99ZZ")
            st_c = await b.btn_state("Mark collected")
            st_w = await b.btn_state("Waive")
            check("D3-oneway-buttons-disabled", st_c.get("found") and st_c.get("disabled")
                  and st_w.get("found") and st_w.get("disabled"), json.dumps([st_c, st_w]))
            check("D4-disabled-title-already-set", st_c.get("title") == LOCALE["connector.flow.action_done"],
                  json.dumps(st_c))
            await b.js("[...document.querySelectorAll('button')].find(x=>x.innerText.trim().toLowerCase()=='mark collected')?.click()")
            await asyncio.sleep(0.6)
            acts = json.loads(await b.dump("__P_ACTIONS__") or "[]")
            check("D5-disabled-click-no-action", len(acts) == 0, str(acts)[:150])
            await b.shot("06-one-way-disabled.png")
        finally:
            await b.close()

        # ---------- Boot D: Öffnen-Button + Legacy-ftabs ----------
        boot_nr += 1
        b = Boot(boot_nr); await b.open(boot_src(DATA_A))
        try:
            await b.key("F7", 118); await asyncio.sleep(1.4)
            tip = LOCALE["connector.scan_open_tip"]
            st = await b.js(f"(()=>{{const b=document.querySelector('button[title={json.dumps(tip)}]');"
                            "return b?{found:true, disabled:b.disabled}:{found:false};})()")
            check("G3a-open-button-enabled", st.get("found") and not st.get("disabled"), json.dumps(st))
            chips = await b.js("[...document.querySelectorAll('.cscan-status')].map(x=>x.innerText.trim().toLowerCase())")
            check("G3b-status-chips", chips == [LOCALE["connector.scan_status_pending"].lower(),
                                                LOCALE["connector.scan_status_done"].lower()], str(chips))
            await b.shot("07-scan-landing.png")
            await b.js(f"document.querySelector('button[title={json.dumps(tip)}]').click()")
            await asyncio.sleep(1.3)
            ks = json.loads(await b.dump("__P_KEYSTORE__") or "[]")
            fl = json.loads(await b.dump("__P_FLOWS__") or "[]")
            check("G1-keystore-find-args", len(ks) == 1 and ks[0].get("id") == "sclm" and ks[0].get("sid") == "s1", str(ks))
            check("G2-flow-open-with-saved-code", len(fl) == 1 and fl[0].get("code") == "ABC123", str(fl))
            body = (await b.js("document.body.innerText") or "").lower()
            check("G4-flow-opens-wishes", "mark wishes" in body, body[:150])
            click = await b.click_text("Creator", selector=".connector-tabs button")
            check("H1a-creator-tab-reachable", click == "OK", str(click))
            await asyncio.sleep(0.9)
            ftabs = await b.js("document.querySelectorAll('.ftabs').length")
            check("H1b-legacy-creator-keeps-ftabs", ftabs >= 1, f"ftabs={ftabs}")
            body = (await b.js("document.body.innerText") or "").lower()
            check("H2-creator-entry-form", "create raffle" in body, body[:160])
            await b.shot("08-creator-legacy-tabs.png")
        finally:
            await b.close()

        print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
        if FAIL:
            print("FAILED:", ", ".join(FAIL))
            return 1
        return 0
    finally:
        server.terminate()
        try:
            await asyncio.wait_for(server.wait(), 5)
        except Exception:
            server.kill()


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
