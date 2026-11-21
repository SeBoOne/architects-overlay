#!/usr/bin/env python3
"""Diagnose-Probe (t_9107f006): A03-Sniper-Rifle Live-Datenpfad im Headless-Webview.

Stub liefert EXAKT die Antworten des echten Backends:
  - strata_craft_cost  = fixtures-live/craft-cost.a03-sniper-rifle.live.json
  - scwiki_craft_detail Blueprint = Live-Fetch /tmp/a03-live-bp.json (Rust-serialisiert)
  - scmdb_quality_snaps = M8-R2-Quelle (SCMDB qualityBands; leer → 500-only)
Erwartung laut Live-Screenshot (Omar): Iron/Taranite-Regler ja, Hadanite-Regler nein.
Ziel: zeigen, ob der Regler für Hadanite im Frontend rendert und warum (nicht).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(__file__).resolve().parents[2]  # wt/
OUT = pathlib.Path(os.environ.get("SHOT_DIR", "/tmp/t_9107f006-probe"))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8899/"

# smoke-stub.js muss VOR dem eigenen Stub geladen werden — er definiert
# window.__TAURI_INTERNALS__ (Headless-Chromium hat kein Tauri-Backend).
STUB_BASE = (ROOT / "docs/scripts/smoke-stub.js").read_text()

CRAFT = json.loads((ROOT / "crates/strata-client/tests/fixtures-live/craft-cost.a03-sniper-rifle.live.json").read_text())
BP = json.loads(pathlib.Path("/tmp/a03-live-bp.json").read_text())

# Snap-Quelle wie Backend quality_snaps_inner (SCMDB-Bänder oder leer →
# emulieren wir separat): Input-UUID = Raffinat/Mineral/Item-UUIDs → 0/404.
SNAPS: dict[str, list[int]] = {}

STUB = """
(function () {
  const CRAFT = %CRAFT%;
  const BP = %BP%;
  const SNAPS = %SNAPS%;
  window.__SNAP_CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_items') return { items: [
      { id: '6b1fb5e8-f9ec-49bc-befb-c81b7c08c5ea', name: 'A03 Sniper Rifle', slug: 'a03-sniper-rifle', category: 'FPS Weapon' },
      // t_9107f006: echter SC-Wiki-Dup — "Antium Core Jet" zweimal (unterschiedliche IDs).
      { id: 'dup-antium-1', name: 'Antium Core Jet', slug: 'antium-core-jet', category: 'FPS Weapon' },
      { id: 'dup-antium-2', name: 'Antium Core Jet', slug: 'antium-core-jet', category: 'FPS Weapon' }
    ], count: 3, attribution: 'Data from celd.space / Strata Mining Tools' };
    if (cmd === 'strata_craft_cost') return CRAFT;
    if (cmd === 'scwiki_craft_detail') return {
      item: { name: 'A03 Sniper Rifle', item_type: 'WeaponGun', size: null, grade: null, class: null,
              mass: null, description_de: null, description_en: null,
              blueprint_uuids: ['eba027dc-b709-4911-bed5-4bc0ae9c8a0f'], vehicle_weapon: null, ammunition: null },
      blueprint: BP,
      attribution: 'Data from star-citizen.wiki'
    };
    if (cmd === 'scmdb_quality_snaps') {
      window.__SNAP_CALLS.push(a);
      return (a.uuids || []).map((u) => ({ uuid: u, values: SNAPS[u] || [] }));
    }
    return orig(cmd, args);
  };
  window.craftDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.sliderRows = () => [...document.querySelectorAll('.crafting .cmq-slider')].map((r) => ({
    mat: r.previousElementSibling?.querySelector('.cm-name')?.textContent.trim(),
    val: r.querySelector('.cmq-val')?.textContent.trim(),
    snapBtn: r.querySelector('.cmq-snap')?.textContent.trim() || null,
  }));
  window.matRows = () => [...document.querySelectorAll('.crafting .cmats .cmat')].map((r) => ({
    name: r.querySelector('.cm-name')?.textContent.trim(),
    qty: r.querySelector('.cm-qty')?.textContent.trim(),
  }));
  window.aspectsDump = () => {
    return [...document.querySelectorAll('.crafting .cmat')].map((r) => r.querySelector('.cm-name')?.textContent.trim());
  };
  window.itemRows = () => [...document.querySelectorAll('.crafting .crow-main')]
    .map((x) => x.textContent.trim());
  window.dismantleTexts = () => [...document.querySelectorAll('.crafting .cdi')]
    .map((x) => x.textContent.trim());
  window.openCraft = (label) => {
    const b = [...document.querySelectorAll('.crafting .crow-main')].find((x) => x.textContent.includes(label));
    if (!b) return false;
    b.click();
    return true;
  };
  window.pressKey = (key) => { window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true })); };
})();
""".replace("%CRAFT%", json.dumps(CRAFT)).replace("%BP%", json.dumps(BP)).replace("%SNAPS%", json.dumps(SNAPS))


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8899", "--bind", "127.0.0.1",
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
    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox", "--disable-gpu",
        "--window-size=1600,1000", "--hide-scrollbars",
        "--remote-debugging-port=9249", "--user-data-dir=/tmp/cdp-t9107-diag",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9249/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        raise RuntimeError("CDP: kein Page-Target gefunden (Chromium nicht gestartet?)")
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

        async def js(expr):
            r = ok(await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}))
            if r.get("exceptionDetails"):
                print("JS-ERR", json.dumps(r["exceptionDetails"])[:400])
                return None
            return r.get("result", {}).get("value")

        async def wait_for(expr, timeout=10):
            end = asyncio.get_event_loop().time() + timeout
            while asyncio.get_event_loop().time() < end:
                v = await js(expr)
                if v:
                    return v
                await asyncio.sleep(0.15)
            return None

        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB_BASE + "\n" + STUB})
        await call("Page.enable")
        await call("Page.navigate", {"url": URL})
        await wait_for("document.querySelector('.rail .rentry')", 15)
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        # ---- t_9107f006 Checks (PASS/FAIL) ------------------------------------
        checks: list[tuple[str, bool]] = []

        # 1) Item-Liste: "Antium Core Jet" nur EINMAL (clientseitiger Dedup)
        item_rows = await js("itemRows()")
        antium = [r for r in (item_rows or []) if "Antium Core Jet" in r]
        checks.append(("dedup: Antium Core Jet nur 1x in Liste", len(antium) == 1))
        checks.append(("dedup: Liste enthaelt A03", any("A03 Sniper Rifle" in r for r in (item_rows or []))))

        await js("openCraft('A03 Sniper Rifle')")
        await wait_for("craftDetailOpen()", 8)
        await asyncio.sleep(1.0)

        mats = await js("matRows()")
        sliders = await js("sliderRows()")
        dism = await js("dismantleTexts()")

        # 2) Regler einheitsunabhaengig: ALLE 4 Materialien haben eine Regler-Zeile
        for mat in ("Iron", "Taranite", "Hadanite", "Hephaestanite"):
            checks.append((f"slider: {mat} hat Regler-Zeile", any(s.get("mat") == mat for s in (sliders or []))))

        # 3) Snap-Toggle IMMER sichtbar an der Regler-Zeile (auch ohne Quant-Werte)
        for mat in ("Iron", "Taranite", "Hadanite", "Hephaestanite"):
            row = next((s for s in (sliders or []) if s.get("mat") == mat), None)
            checks.append((f"snap-toggle: {mat} sichtbar", bool(row and row.get("snapBtn"))))

        # 4) Dismantle-Zeile: Leerzeichen zwischen Name und Quantitaet
        dtext = " ".join(dism or [])
        checks.append(("dismantle: Leerzeichen 'Hephaestanite 0.01 SCU'", "Hephaestanite 0.01 SCU" in dtext))
        checks.append(("dismantle: kein 'Hephaestanite0.01'", "Hephaestanite0.01" not in dtext))

        npass = sum(1 for _, okk in checks if okk)
        for name, okk in checks:
            print(("PASS" if okk else "FAIL"), "-", name)
        print(f"== {npass}/{len(checks)} PASS ==")
        print("matRows:", json.dumps(mats, indent=1))
        print("sliderRows:", json.dumps(sliders, indent=1))
        print("dismantle:", json.dumps(dism, indent=1))
        data = ok(await call("Page.captureScreenshot", {"format": "png"}))["data"]
        (OUT / "rest-politur-a03.png").write_bytes(__import__("base64").b64decode(data))
        print("shot", OUT / "rest-politur-a03.png")
        if npass != len(checks):
            raise SystemExit(1)

    for p in (proc, server):
        try:
            p.kill()
        except ProcessLookupError:
            pass


asyncio.run(main())
