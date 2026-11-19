#!/usr/bin/env python3
"""M8-K6 probe (t_f7a29367) — Echter Qualitäts-Stat-Rechner, self-contained.

Gates:
  1. Craft-Detail OHNE Blueprint-Match: Stat-Rechner-Block bleibt WEG
     (degradiert still, kein Hard-Fail — Plan: "Fehlender Match degradiert")
  2. Craft-Detail MIT Blueprint-Match (Live-Beleg-Shape: Integrity 0.9→1.1,
     Impact Force 0.95→1.05, Q-Range 0..1000): Block erscheint, Reihenfolge
     folgt summary_properties (Integrity vor Impact Force)
  3. Slider Q=0 → −10 % / −5 % (Ampel rot = schlechter bei better_when higher)
  4. Slider Q=1000 → +10 % / +5 % (grün = besser)
  5. min_quality-Badge + Erreichbarkeit: min_quality 500 → bei Q0 "nicht
     erreichbar" (rot), bei Q1000 "erreichbar" (grün)
  6. Slider-Änderung reaktiv (Q500 → ±0 % neutral)

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Rechenmodell live gegen api.star-citizen.wiki verifiziert (Omnisky III
Cannon, BP_CRAFT_AMRS_LaserCannon_S1, 4.10.3-LIVE.13165946, 2026-11-19).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_f7a29367")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m8k6-quality-stats", ignore_errors=True)


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS {name}")
    else:
        FAIL += 1
        print(f"  FAIL {name}  {detail}")


WRAP = r"""
(function () {
  const ITEMS = { items: [
    { id: 'item-0', name: 'Plain Cannon', slug: 'plain-cannon', category: 'weapon' },
    { id: 'item-1', name: 'Quality Cannon', slug: 'quality-cannon', category: 'weapon' }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };

  // Ohne SC-Wiki-Blueprint-Match: Stat-Rechner-Block bleibt weg.
  const CRAFT_NOBP = {
    item: { id: 'item-0', name: 'Plain Cannon', category: 'weapon' },
    craftTime: 960,
    costs: [
      { material: 'Agricium', oreId: null, quantity: 0.36, unit: 'SCU', minQuality: 1 },
      { material: 'Hadanite', oreId: null, quantity: 7, unit: 'items', minQuality: null }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };

  // Mit Blueprint-Match — Shape exakt nach SC-Wiki-Live-Beleg
  // (aspects[].input + aspects[].modifiers, summary_properties).
  const CRAFT_BP = {
    item: { id: 'item-1', name: 'Quality Cannon', category: 'weapon' },
    craftTime: 540,
    costs: [
      { material: 'Agricium', oreId: null, quantity: 0.36, unit: 'SCU', minQuality: 500 },
      { material: 'Hadanite', oreId: null, quantity: 7, unit: 'items', minQuality: 1 }
    ],
    blueprintAspects: [
      { key: 'FRAME', name: 'Frame', required_count: 1,
        input: { name: 'Agricium', min_quality: 500, quantity_scu: 0.36 },
        modifiers: [
          { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher',
            quality_range: { min: 0, max: 1000 },
            modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 },
            value_range_type: 'linear' }
        ] },
      { key: 'EMITTER', name: 'Emitter', required_count: 1,
        input: { name: 'Hadanite', min_quality: 1, quantity: 7 },
        modifiers: [
          { property_key: 'weapon_damage', label: 'Impact Force', better_when: 'higher',
            quality_range: { min: 0, max: 1000 },
            modifier_range: { at_min_quality: 0.95, at_max_quality: 1.05 },
            value_range_type: 'linear' }
        ] }
    ],
    blueprintSummary: [
      { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher' },
      { property_key: 'weapon_damage', label: 'Impact Force', better_when: 'higher' }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };

  window.__STRATA_CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4 };
    if (cmd === 'strata_items') { window.__STRATA_CALLS.push({ cmd }); return ITEMS; }
    if (cmd === 'strata_craft_cost') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      return a.item === 'item-1' ? CRAFT_BP : CRAFT_NOBP;
    }
    return orig(cmd, args);
  };
  window.craftDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.qsPresent = () => !!document.querySelector('.crafting .qs');
  window.qsProps = () => [...document.querySelectorAll('.crafting .qs-prop')].map((r) => ({
    name: r.querySelector('.qs-prop-name')?.textContent.trim(),
    val: r.querySelector('.qs-prop-val')?.textContent.trim(),
    cls: r.className
  }));
  window.qsAspects = () => [...document.querySelectorAll('.crafting .qs-aspect')].map((r) => ({
    name: r.querySelector('.qs-aspect-name')?.textContent.trim(),
    mat: r.querySelector('.qs-mat')?.textContent.trim(),
    mq: r.querySelector('.qs-mq')?.textContent.trim(),
    mqCls: r.querySelector('.qs-mq')?.className || '',
    reach: r.querySelector('.qs-reach')?.textContent.trim(),
    reachCls: r.querySelector('.qs-reach')?.className || ''
  }));
  window.setTarget = (v) => {
    const s = document.querySelector('.crafting .cp-slider');
    if (!s) return false;
    s.value = String(v);
    s.dispatchEvent(new Event('input', { bubbles: true }));
    s.dispatchEvent(new Event('change', { bubbles: true }));
    return true;
  };
  window.openCraft = (label) => {
    const b = [...document.querySelectorAll('.crafting .crow-main')].find((x) => x.textContent.includes(label));
    if (!b) return false;
    b.click();
    return true;
  };
  window.pressKey = (key) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  };
})();
"""


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8797", "--bind", "127.0.0.1",
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
        "--window-size=1600,1000", "--hide-scrollbars",
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-m8k6-quality-stats",
        "about:blank",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(80):
        try:
            with urllib.request.urlopen("http://127.0.0.1:9234/json/list", timeout=1) as r:
                pages = [t for t in json.load(r) if t.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            pass
        await asyncio.sleep(0.25)
    if ws is None:
        raise RuntimeError("CDP page target did not come up")

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

        async def wrap(expr):
            return ok(await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB + "\n" + expr}))

        async def shot(name):
            data = ok(await call("Page.captureScreenshot", {"format": "png"}))["data"]
            (OUT / name).write_bytes(__import__("base64").b64decode(data))
            print(f"  shot {OUT / name}")

        await wrap(WRAP)
        ok(await call("Page.enable"))
        ok(await call("Page.navigate", {"url": URL}))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        # ---- Gate 1: ohne Blueprint-Match bleibt der Block weg ----
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("openCraft('Plain Cannon')")
        await wait_for("craftDetailOpen()", 8)
        check("1a Detail offen (Plain Cannon)", await js("craftDetailOpen()") is True)
        check("1b Kein Stat-Rechner ohne Blueprint-Match", await js("qsPresent()") is False)
        await shot("m8-k6-detail-nobp.png")

        # ---- Gate 2-6: mit Blueprint-Match ----
        await js("window.pressKey('Escape')")
        await asyncio.sleep(0.3)
        await js("openCraft('Quality Cannon')")
        await wait_for("craftDetailOpen() && qsPresent()", 8)
        check("2a Detail + Stat-Rechner-Block offen", await js("craftDetailOpen() && qsPresent()") is True)
        props = await js("qsProps()")
        check("2b Reihenfolge folgt summary_properties (Integrity vor Impact Force)",
              props and [p["name"] for p in props][:2] == ["Integrity", "Impact Force"], str(props))
        await shot("m8-k6-detail-bp-q0.png")

        # Slider startet bei 0 (neutraler Start pro Detail-Öffnung)
        # Gate 3: Q0 → −10 % / −5 %, rot (schlechter)
        props = await js("qsProps()")
        check("3a Q0: Integrity −10 %", props and props[0]["val"] == "-10 %", str(props))
        check("3b Q0: Impact Force −5 %", props and props[1]["val"] == "-5 %", str(props))
        check("3c Q0: Ampel rot (is-worse)",
              props and all("is-worse" in p["cls"] for p in props), str(props))

        # Gate 5a: min_quality 500 bei Q0 → nicht erreichbar
        aspects = await js("qsAspects()")
        check("5a Frame/Agricium Badge 500, Q0 nicht erreichbar (rot)",
              aspects and aspects[0]["mq"] == "500" and "is-fail" in aspects[0]["mqCls"]
              and "not reachable" in aspects[0]["reach"] and "is-fail" in aspects[0]["reachCls"],
              str(aspects))
        check("5b Emitter/Hadanite Badge 1, Q0 korrekt nicht erreichbar (min_quality 1 > Q0)",
              aspects and aspects[1]["mq"] == "1" and "is-fail" in aspects[1]["mqCls"]
              and "not reachable" in aspects[1]["reach"] and "is-fail" in aspects[1]["reachCls"],
              str(aspects))

        # Gate 4: Q1000 → +10 % / +5 %, grün
        await js("setTarget(1000)")
        await asyncio.sleep(0.3)
        props = await js("qsProps()")
        check("4a Q1000: Integrity +10 %", props and props[0]["val"] == "+10 %", str(props))
        check("4b Q1000: Impact Force +5 %", props and props[1]["val"] == "+5 %", str(props))
        check("4c Q1000: Ampel grün (is-better)",
              props and all("is-better" in p["cls"] for p in props), str(props))
        # Gate 5b: min_quality 500 bei Q1000 → erreichbar
        aspects = await js("qsAspects()")
        check("5c Q1000: Frame 500 erreichbar (grün)",
              aspects and "is-ok" in aspects[0]["mqCls"] and "not reachable" not in aspects[0]["reach"],
              str(aspects))
        await shot("m8-k6-detail-bp-q1000.png")

        # Gate 6: Q500 → ±0 % neutral (kein better/kein worse)
        await js("setTarget(500)")
        await asyncio.sleep(0.3)
        props = await js("qsProps()")
        check("6a Q500: ±0 % neutral", props and props[0]["val"] == "0 %" and "is-better" not in props[0]["cls"]
              and "is-worse" not in props[0]["cls"], str(props))
        await shot("m8-k6-detail-bp-q500.png")

    proc.kill()
    server.kill()
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


asyncio.run(main())
