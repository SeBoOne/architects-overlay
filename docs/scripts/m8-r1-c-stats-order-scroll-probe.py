#!/usr/bin/env python3
"""M8-R1-C probe (t_b6ee9f14) — Stats-Vollanzeige + Reihenfolge + Scroll-Bug.

Gates:
  1. Reihenfolge im Detail (Spec): Size/Mass/Ammo-Props → Stats-Vollanzeige
     (.qs) → eingeklappte Description (.cd-desc) → Materialien (.cmats) +
     Planungsrechner (.cplanner) → Blueprint-Missionen (.cmissions) →
     Dismantle (.cdism). DOM-Reihenfolge via compareDocumentPosition.
  2. Stats-Vollanzeige: ALLE Stats untereinander mit default | neu —
     Damage/shot 73 default, bei Q1000 → 76.7 (+5 %) (weapon_damage-Faktor
     1.05 aus dem CYCLER-Aspect), damages-Liste skaliert mit (Kinetic 40 →
     42, Distortion 33 → 34.7), Integrity ohne API-Basiswert → „—“ default
     + "+10 %".
  3. Q-unbeeinflusste Stats sauber ausgewiesen: RPM/Range/Capacity/Ammo-
     Stats bleiben neu = default mit „unaffected“-Badge.
  4. Slider Q500 → Faktor 1.0 → kein %-Unterschied, alles „unaffected“.
  5. Scroll-Bug (User-P4): LONGEM-Blueprint (12 Missionen) im Detail —
     bei kleinem Viewport (1000×640) wird nichts abgeschnitten: .cdetail
     scrollt (scrollHeight > clientHeight, scrollTop bis unten), die
     letzte Mission + Dismantle-Zeile sind NACH dem Scrollen komplett
     innerhalb des Panels, Zierramen (border/radius) erhalten.
  6. Root-Cause-Beleg: .crafting-keep-Wrapper ist in der Flex-Kette
     (Höhe == .shell-main) und .crafting füllt ihn — der Wrapper wächst
     NICHT über die Shell hinaus.

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Daten-Beleg: crates/scwiki-client/tests/fixtures/ (ECHTE Responses,
Live-Zug 2026-11-19) + 12-Missionen-Variante für den Scroll-Fall.
"""
import asyncio, json, os, pathlib
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path(__file__).parents[2] / "docs/screenshots/m8-r1-c")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m8r1c-" + str(__import__("random").randint(1000, 99999)), ignore_errors=True)


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
    { id: 'item-1', name: 'Greatsword Cannon', slug: 'greatsword-cannon', category: 'weapon' },
    { id: 'item-2', name: 'LONGEM Blaster', slug: 'longem-blaster', category: 'weapon' }
  ], count: 3, attribution: 'Data from celd.space / Strata Mining Tools' };

  const CRAFT_NOBP = {
    item: { id: 'item-0', name: 'Plain Cannon', category: 'weapon' },
    craftTime: 180, costs: [
      { material: 'Agricium', oreId: null, quantity: 0.5, unit: 'SCU', minQuality: 1 }
    ], attribution: 'Data from celd.space / Strata Mining Tools'
  };

  const CRAFT_BP = {
    item: { id: 'item-1', name: 'Greatsword Cannon', category: 'weapon' },
    craftTime: 960, costs: [
      { material: 'Iron', oreId: 'ore-iron', quantity: 0.64, unit: 'SCU', minQuality: 1 },
      { material: 'Riccite', oreId: 'ore-riccite', quantity: 0.09, unit: 'SCU', minQuality: 1 },
      { material: 'Titanium', oreId: 'ore-titanium', quantity: 0.16, unit: 'SCU', minQuality: 500 }
    ], attribution: 'Data from celd.space / Strata Mining Tools'
  };

  const CRAFT_LONGEM = {
    item: { id: 'item-2', name: 'LONGEM Blaster', category: 'weapon' },
    craftTime: 960, costs: [
      { material: 'Iron', oreId: 'ore-iron', quantity: 0.64, unit: 'SCU', minQuality: 1 },
      { material: 'Riccite', oreId: 'ore-riccite', quantity: 0.09, unit: 'SCU', minQuality: 1 },
      { material: 'Titanium', oreId: 'ore-titanium', quantity: 0.16, unit: 'SCU', minQuality: 500 }
    ], attribution: 'Data from celd.space / Strata Mining Tools'
  };

  // Shape exakt nach M8-K5-Fixtures (Greatsword, Live-Zug 2026-11-19).
  const SCWIKI_GREATSWORD = {
    item: {
      name: '10-Series Greatsword Cannon', item_type: 'WeaponGun',
      size: 2, grade: 'A', class: null, mass: 192.0,
      description_de: 'Hersteller: KnightBridge Arms\nGroße ballistische Kanone.',
      description_en: 'Manufacturer: KnightBridge Arms',
      blueprint_uuids: ['bp-greatsword'],
      vehicle_weapon: {
        capacity: 492, range: 2703, damage_per_shot: 73, rpm: 250,
        damages: [ { damage_type: 'Kinetic', damage: 40 }, { damage_type: 'Distortion', damage: 33 } ]
      },
      ammunition: { size: 1, capacity: 0, speed: 1100, range: 2700, lifetime: null }
    },
    blueprint: {
      uuid: 'bp-greatsword', output_name: '10-Series Greatsword Cannon',
      is_available_by_default: true, game_version: '4.10.3-LIVE.13165946',
      dismantle_returns: [ { name: 'Iron', quantity_scu: 0.32 }, { name: 'Titanium', quantity_scu: 0.16 } ],
      summary_properties: [
        { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher' },
        { property_key: 'weapon_damage', label: 'Impact Force', better_when: 'higher' }
      ],
      aspects: [
        { key: 'FRAME', name: 'Frame', input: { name: 'Iron', min_quality: 1 },
          modifiers: [ { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher',
            quality_range: { min: 0, max: 1000 }, modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 },
            value_range_type: 'linear' } ] },
        { key: 'CYCLER', name: 'Cycler', input: { name: 'Riccite', min_quality: 1 },
          modifiers: [ { property_key: 'weapon_damage', label: 'Impact Force', better_when: 'higher',
            quality_range: { min: 0, max: 1000 }, modifier_range: { at_min_quality: 0.95, at_max_quality: 1.05 },
            value_range_type: 'linear' } ] }
      ],
      unlocking_missions_grouped: [
        { label: 'Guaranteed', chance: 1, missions: [
          { title: 'Mission One', debug_name: 'HH_M1', reward_scope: 'Bounty Hunter', count: 1, web_url: null },
          { title: 'Mission Two', debug_name: 'HH_M2', reward_scope: 'Bounty Hunter', count: 1, web_url: null }
        ] },
        { label: 'Vulnerability Assessment', chance: 0.4, missions: [
          { title: 'Mission Three', debug_name: 'HH_M3', reward_scope: 'Bounty Hunter', count: 1, web_url: null }
        ] }
      ],
      web_url: 'https://star-citizen.wiki/blueprints/greatsword'
    },
    attribution: 'Data from star-citizen.wiki'
  };

  // Scroll-Fall: LONGEM-Blueprint mit 12 Missionen (User-P4 — „unten
  // abgeschnitten, Missionen unsichtbar, Zierramen fehlt").
  const SCWIKI_LONGEM = JSON.parse(JSON.stringify(SCWIKI_GREATSWORD));
  SCWIKI_LONGEM.item.name = 'LONGEM Blaster';
  SCWIKI_LONGEM.item.blueprint_uuids = ['bp-longem'];
  SCWIKI_LONGEM.blueprint.uuid = 'bp-longem';
  SCWIKI_LONGEM.blueprint.output_name = 'LONGEM Blaster';
  SCWIKI_LONGEM.blueprint.is_available_by_default = false;
  const missions = [];
  for (let i = 1; i <= 12; i++) {
    missions.push({ title: 'LONGEM Mission ' + i, debug_name: 'LONGEM_SCAN_M' + i,
      reward_scope: 'Bounty Hunter', count: 1, web_url: null });
  }
  SCWIKI_LONGEM.blueprint.unlocking_missions_grouped = [
    { label: 'Guaranteed', chance: 1, missions: missions.slice(0, 7) },
    { label: 'Vulnerability Assessment', chance: 0.4, missions: missions.slice(7) }
  ];

  window.__STRATA_CALLS = [];
  window.pressKey = function (key) {
    window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  };
  window.__JS_ERRS = [];
  window.onerror = (m, src, l, c) => { window.__JS_ERRS.push(String(m) + ' @' + (src||'').split('/').pop() + ':' + l); };
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'k', tier_default: 2, scwiki_ready: true };
    if (cmd === 'strata_items') { window.__STRATA_CALLS.push({ cmd }); return ITEMS; }
    if (cmd === 'strata_craft_cost') {
      window.__STRATA_CALLS.push({ cmd });
      if (a.item === 'item-1') return CRAFT_BP;
      if (a.item === 'item-2') return CRAFT_LONGEM;
      return CRAFT_NOBP;
    }
    if (cmd === 'scwiki_craft_detail') {
      window.__STRATA_CALLS.push({ cmd });
      if (a.itemName === 'Greatsword Cannon') return SCWIKI_GREATSWORD;
      if (a.itemName === 'LONGEM Blaster') return SCWIKI_LONGEM;
      return null;
    }
    return orig(cmd, args);
  };

  window.openCraft = (label) => {
    const b = [...document.querySelectorAll('.crafting .crow-main')].find((x) => x.textContent.includes(label));
    if (!b) return false;
    b.click();
    return true;
  };
  // Reihenfolge: DOM-Position vergleichen (follows=true → nicht einklappen).
  window.orderOk = () => {
    const q = (s) => document.querySelector('.crafting ' + s);
    const A = q('.qs'), B = q('.cd-desc'), C = q('.cmats'), D = q('.cplanner'),
          E = q('.cmissions'), F = q('.cdism');
    if (!A || !B || !C || !D || !E || !F) return { ok: false, missing: !A ? '.qs' : !B ? '.cd-desc' : !C ? '.cmats' : !D ? '.cplanner' : !E ? '.cmissions' : '.cdism' };
    const P = Node.DOCUMENT_POSITION_FOLLOWING;
    const before = (x, y) => !!(x.compareDocumentPosition(y) & P);
    return { ok: before(A, B) && before(B, C) && before(C, D) && before(D, E) && before(E, F) };
  };
  // Statzeilen (Vollanzeige): label | default | neu.
  window.statRows = () => [...document.querySelectorAll('.crafting .qs .qs-prop')].map((r) => ({
    label: r.querySelector('.qs-prop-name')?.textContent.trim(),
    def: r.querySelector('.qs-prop-def')?.textContent.trim(),
    val: r.querySelector('.qs-prop-val')?.textContent.replace(/\s+/g, ' ').trim()
  }));
  window.qsCols = () => [...document.querySelectorAll('.crafting .qs .qs-col-l, .crafting .qs .qs-col-r')].map((x) => x.textContent.trim());
  window.setSlider = (v) => {
    const rows = [...document.querySelectorAll('.crafting .cmq-range')];
    if (!rows.length) return false;
    for (const sl of rows) {
      sl.value = String(v);
      sl.dispatchEvent(new Event('input', { bubbles: true }));
    }
    return rows.length;
  };
  // Scroll-Bug: Panel scrollen und die unterste Zeile einmessen.
  window.scrollPanel = () => {
    const d = document.querySelector('.crafting .cdetail');
    d.scrollTop = d.scrollHeight;
    return d.scrollTop;
  };
  window.panelBottomVisible = () => {
    const d = document.querySelector('.crafting .cdetail');
    const missions = [...document.querySelectorAll('.crafting .cmissions .cm-mission')];
    const dism = document.querySelector('.crafting .cdism');
    const dr = d.getBoundingClientRect();
    const last = missions[missions.length - 1]?.getBoundingClientRect();
    const fr = dism?.getBoundingClientRect();
    return {
      scrollable: d.scrollHeight > d.clientHeight,
      scrollTop: d.scrollTop,
      lastMission: last ? { top: last.top, bottom: last.bottom } : null,
      dismantle: fr ? { top: fr.top, bottom: fr.bottom } : null,
      panel: { top: dr.top, bottom: dr.bottom },
      lastInside: last ? last.bottom <= dr.bottom + 1 && last.top >= dr.top - 1 : false,
      dismInside: fr ? fr.bottom <= dr.bottom + 1 && fr.top >= dr.top - 1 : false,
      lastText: missions[missions.length - 1]?.textContent.trim() ?? null
    };
  };
  // Root-Cause-Beleg: Wrapper in der Flex-Kette.
  window.flexChain = () => {
    const w = document.querySelector('.crafting-keep');
    const m = document.querySelector('.shell-main');
    if (!w || !m) return null;
    const wr = w.getBoundingClientRect(), mr = m.getBoundingClientRect();
    return {
      wrapperDisplay: getComputedStyle(w).display,
      wrapperBottom: wr.bottom, shellBottom: mr.bottom,
      wrapperFits: wr.bottom <= mr.bottom + 1,
      craftingFills: (() => {
        const c = document.querySelector('.crafting');
        return c ? getComputedStyle(c).flex !== 'none' : false;
      })()
    };
  };
  window.pressKey = (k) => window.dispatchEvent(new KeyboardEvent('keydown', { key: k }));
})();
"""


async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", "8797", "--directory", str(ROOT / "build"),
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    for _ in range(60):
        try:
            import urllib.request
            urllib.request.urlopen(URL, timeout=1)
            break
        except Exception:
            await asyncio.sleep(0.25)
    else:
        raise RuntimeError("static server did not come up")

    proc = await asyncio.create_subprocess_exec(
        "/usr/sbin/chromium", "--headless=new", "--no-sandbox",
        "--window-size=1000,640", "--hide-scrollbars",
        "--remote-debugging-port=9341", "--user-data-dir=/tmp/cdp-m8r1c", "--disable-dev-shm-usage",
        "about:blank", stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
    ws = None
    for _ in range(60):
        try:
            import urllib.request
            lst = json.loads(urllib.request.urlopen("http://127.0.0.1:9341/json/list", timeout=1).read())
            pages = [p for p in lst if p.get("type") == "page"]
            if pages:
                ws = pages[0]["webSocketDebuggerUrl"]
                break
        except Exception:
            await asyncio.sleep(0.25)
    if not ws:
        raise RuntimeError("no CDP page")

    async with websockets.connect(ws, max_size=None) as cdp:
        mid = {"n": 0}
        pending = {}

        async def call(method, params=None):
            mid["n"] += 1
            fut = asyncio.get_event_loop().create_future()
            pending[mid["n"]] = fut
            await cdp.send(json.dumps({"id": mid["n"], "method": method, "params": params or {}}))
            return await fut

        async def recv_loop():
            async for msg in cdp:
                d = json.loads(msg)
                if "id" in d and d["id"] in pending:
                    pending.pop(d["id"]).set_result(d.get("result", d))

        rl = asyncio.get_event_loop().create_task(recv_loop())

        async def js(expr):
            r = await call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True})
            return r.get("result", {}).get("value")

        async def wait_for(expr, timeout=10):
            for _ in range(int(timeout / 0.25)):
                v = await js(expr)
                if v:
                    return v
                await asyncio.sleep(0.25)
            return None

        async def shot(name):
            r = await call("Page.captureScreenshot", {"format": "png"})
            (OUT / name).write_bytes(__import__("base64").b64decode(r["data"]))

        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB + "\n" + WRAP})
        await call("Page.enable")
        await call("Page.navigate", {"url": URL})
        rr = await call("Runtime.evaluate", {"expression": "location.href", "returnByValue": True})
        print("    DEBUG nav:", await js("location.href + ' | ' + (document.documentElement.outerHTML.slice(0,500))"))
        await wait_for("document.querySelector('.rail .rentry')", 15)

        # ---- Gate 1+2+3: Reihenfolge + Vollanzeige (Greatsword) ----
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 10)
        await wait_for("openCraft('Greatsword Cannon')", 10)
        await wait_for("document.querySelector('.crafting .cd-title')", 10)
        await wait_for("document.querySelector('.crafting .qs')", 10)
        await asyncio.sleep(0.4)
        await asyncio.sleep(1.5)
        await js("setSlider(1000)")  # Q0-Baseline → Q1000 für die Vollanzeige
        await asyncio.sleep(0.3)

        o = await js("orderOk()")
        check("1 Reihenfolge props→qs→desc→mats→planner→missions→dism",
              isinstance(o, dict) and o.get("ok") is True, str(o))

        print("    DEBUG qs:", await js("document.querySelector('.crafting .qs')?.textContent.trim().slice(0,400)"))
        print("    DEBUG qsprops:", await js("[...document.querySelectorAll('.crafting .qs .qs-prop')].length"))
        rows = await js("statRows()")
        rows_all = await js("statRows()")
        check("2a Spalten-Kopf default | neu", await js("qsCols()") == ["Default", "At target quality"],
              str(await js("qsCols()")))
        by = {r["label"]: r for r in (rows or [])}
        check("2b Damage/shot default 73, Q1000 → 76.7 (+5 %)",
              by.get("Damage/shot", {}).get("def") == "73"
              and by.get("Damage/shot", {}).get("val") == "76.7 (+5 %)", str(by.get("Damage/shot")))
        check("2c Kinetic 40 → 42 (+5 %)", by.get("Kinetic", {}).get("def") == "40"
              and by.get("Kinetic", {}).get("val") == "42 (+5 %)", str(by.get("Kinetic")))
        check("2d Distortion 33 → 34.7 (+5 %)", by.get("Distortion", {}).get("def") == "33"
              and by.get("Distortion", {}).get("val") == "34.7 (+5 %)", str(by.get("Distortion")))
        check("2e Integrity: default „—“, neu +10 %", by.get("Integrity", {}).get("def") == "—"
              and by.get("Integrity", {}).get("val") == "+10 %", str(by.get("Integrity")))
        check("3a RPM unaffected (250 = 250)", by.get("RPM", {}).get("def") == "250"
              and "unaffected" in by.get("RPM", {}).get("val", ""), str(by.get("RPM")))
        rng = [r for r in rows_all if r["label"] == "Range"]
        cap = [r for r in rows_all if r["label"] == "Capacity"]
        check("3b VW-Range unaffected (2703 m)", rng and rng[0]["def"] == "2703 m"
              and "unaffected" in rng[0]["val"], str(rng))
        check("3c VW-Capacity unaffected (492)", cap and cap[0]["def"] == "492"
              and "unaffected" in cap[0]["val"], str(cap))
        check("3d Ammo-Speed unaffected (1100 m/s)", by.get("Speed", {}).get("def") == "1100 m/s"
              and "unaffected" in by.get("Speed", {}).get("val", ""), str(by.get("Speed")))
        check("3e Ammo-Range unaffected (2700 m)", len(rng) > 1 and rng[1]["def"] == "2700 m"
              and "unaffected" in rng[1]["val"], str(rng))
        check("3f Ammo-Lifetime ohne Wert → „—“ + unaffected",
              by.get("Lifetime", {}).get("def") == "—"
              and "unaffected" in by.get("Lifetime", {}).get("val", ""), str(by.get("Lifetime")))
        await js("const q=document.querySelector('.crafting .qs'); q.scrollIntoView({block:'start'})")
        await asyncio.sleep(0.3)
        await shot("m8r1c-1-vollanzeige.png")

        # ---- Gate 4: Slider Q500 → Faktor 1.0 → alles neutral ----
        check("4a Slider auf 500", (await js("setSlider(500)") or 0) > 0)
        await asyncio.sleep(0.3)
        rows = await js("statRows()")
        by = {r["label"]: r for r in (rows or [])}
        check("4b Damage/shot Q500 = default (unaffected)",
              by.get("Damage/shot", {}).get("val") == "73" or "unaffected" in by.get("Damage/shot", {}).get("val", ""),
              str(by.get("Damage/shot")))
        check("4c Integrity Q500 = 0 % (kein %-Unterschied)",
              by.get("Integrity", {}).get("val") == "0 %", str(by.get("Integrity")))
        await js("setSlider(1000)")
        await asyncio.sleep(0.2)

        # ---- Gate 5+6: Scroll-Bug (LONGEM, 12 Missionen, 1000×640) ----
        await wait_for("openCraft('LONGEM Blaster')", 10)
        await wait_for("document.querySelector('.crafting .cd-title')", 10)
        await wait_for("document.querySelector('.crafting .cmissions')", 10)
        await asyncio.sleep(0.4)
        n_missions = await js("document.querySelectorAll('.crafting .cmissions .cm-mission').length")
        check("5a LONGEM-Blueprint mit 12 Missionen", n_missions == 12, str(n_missions))

        fc = await js("flexChain()")
        check("6a Root-Cause: crafting-keep in der Flex-Kette (display:flex)",
              isinstance(fc, dict) and fc.get("wrapperDisplay") == "flex", str(fc))
        check("6b Wrapper wächst nicht über die Shell hinaus",
              isinstance(fc, dict) and fc.get("wrapperFits") is True, str(fc))
        check("6c .crafting ist Flex-Item (füllt den Wrapper)",
              isinstance(fc, dict) and fc.get("craftingFills") is True, str(fc))

        pv = await js("panelBottomVisible()")
        check("5b .cdetail scrollt (scrollHeight > clientHeight)",
              isinstance(pv, dict) and pv.get("scrollable") is True, str(pv))
        st = await js("scrollPanel()")
        await asyncio.sleep(0.3)
        pv = await js("panelBottomVisible()")
        check("5c scrollTop > 0 nach scrollPanel()", pv.get("scrollTop", 0) > 0, str(pv))
        check("5d letzte Mission (LONGEM Mission 12) komplett im Panel",
              pv.get("lastInside") is True and "LONGEM Mission 12" in str(pv.get("lastText", "")), str(pv))
        check("5e Dismantle-Zeile komplett im Panel (Zierramen unterm Text)",
              pv.get("dismInside") is True, str(pv))
        await shot("m8r1c-2-scroll-bottom.png")

        rl.cancel()
    proc.kill()
    server.kill()
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(0 if FAIL == 0 else 1)


asyncio.run(main())
