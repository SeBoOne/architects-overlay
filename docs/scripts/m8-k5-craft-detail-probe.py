#!/usr/bin/env python3
"""M8-K5 probe (t_03eafbfb) — Craft-Detail Welle 2 (Item-Details + Missionen).

Gates:
  1. Craft-Detail OHNE SC-Wiki-Match: Kopf-Props/Beschreibung/Stat-Block/
     Missions/dismantle-Blöcke bleiben WEG (degradiert still, kein „—"-Prop,
     kein Hard-Fail — Plan: „Fehlender Match degradiert").
  2. Craft-Detail MIT SC-Wiki-Match (Live-Beleg-Shape, Greatsword-Fixture):
     Kopf-Props nur vorhandene Felder (Size S2, Grade A, Mass 192 kg,
     Muni S1 — KEINE Class-Zeile), Standard-Blaupause-Badge
     (is_available_by_default), Beschreibung kompakt klappbar (de_DE).
  3. Stat-Block: vehicle_weapon-Details (Damage/shot 73, RPM 250,
     Range 2703 m, Capacity 492) + damages-Liste mit dmg-Typen.
  4. dismantle_returns klein unter der Materialliste (Iron 0.32 SCU,
     Titanium 0.16 SCU).
  5. Blaupausen-Missionen: „Guaranteed"-Gruppe hervorgehoben (is-guaranteed),
     Chance-Gruppe mit %-Angabe (40 %), reward_scope als Badge, Titel +
     Debug-Name kompakt.
  6. K6-Verdrahtung: blueprintAspects aus dem SC-Wiki-Blueprint füllen den
     QualityStats-Block (Stat-Rechner erscheint).
  7. Attribution-Footer: SC-Wiki-Angabe zusätzlich zur Strata-Angabe.

Backend = Tauri-Stub (docs/scripts/smoke-stub.js) + Override (dieses File).
Daten-Beleg: crates/scwiki-client/tests/fixtures/ (ECHTE Responses,
Live-Zug 2026-11-19, M8-K4).
"""
import asyncio, json, os, pathlib, urllib.request
import websockets

ROOT = pathlib.Path(os.environ.get("SHOT_ROOT", str(pathlib.Path(__file__).resolve().parents[2])))
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(pathlib.Path.home() / ".hermes/profiles/builder/cache/scratch/t_03eafbfb")))
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://127.0.0.1:8797/"

PASS = 0
FAIL = 0

import shutil
shutil.rmtree("/tmp/cdp-m8k5-craft-detail", ignore_errors=True)


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
    { id: 'item-1', name: 'Greatsword Cannon', slug: 'greatsword-cannon', category: 'weapon' }
  ], count: 2, attribution: 'Data from celd.space / Strata Mining Tools' };

  // Ohne SC-Wiki-Match: Welle-2-Blöcke bleiben weg.
  const CRAFT_NOBP = {
    item: { id: 'item-0', name: 'Plain Cannon', category: 'weapon' },
    craftTime: 960,
    costs: [
      { material: 'Agricium', oreId: null, quantity: 0.36, unit: 'SCU', minQuality: 1 },
      { material: 'Hadanite', oreId: null, quantity: 7, unit: 'items', minQuality: null }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };

  const CRAFT_BP = {
    item: { id: 'item-1', name: 'Greatsword Cannon', category: 'weapon' },
    craftTime: 960,
    costs: [
      { material: 'Iron', oreId: 'ore-1', quantity: 0.64, unit: 'SCU', minQuality: 1 },
      { material: 'Riccite', oreId: 'ore-2', quantity: 0.09, unit: 'SCU', minQuality: 1 },
      { material: 'Titanium', oreId: 'ore-3', quantity: 0.27, unit: 'SCU', minQuality: 500 }
    ],
    attribution: 'Data from celd.space / Strata Mining Tools'
  };

  // Shape exakt nach M8-K4-Fixtures (Greatsword, Live-Zug 2026-11-19).
  const SCWIKI_DETAIL = {
    item: {
      name: '10-Series Greatsword Cannon',
      item_type: 'WeaponGun',
      size: 2,
      grade: 'A',
      class: null,
      mass: 192.0,
      description_de: 'Hersteller: KnightBridge Arms\nGröße: S2\nMit seinen leistungsstarken ballistischen Geschossen begleitet dich die S2-S10-Series Greatsword bis zum Ende des Konflikts.',
      description_en: 'Manufacturer: KnightBridge Arms\nSize: S2\nWith its powerful ballistic rounds, the versatile, size two 10-Series Greatsword will see you through to the conflict\'s end.',
      blueprint_uuids: ['b04aff42-d892-4ca9-991e-101c869a7caa'],
      vehicle_weapon: {
        capacity: 492, range: 2703, damage_per_shot: 73, rpm: 250,
        damages: [ { damage_type: 'Kinetic', damage: 40 }, { damage_type: 'Distortion', damage: 33 } ]
      },
      ammunition: { size: 1, capacity: 0, speed: 1100, range: 2700, lifetime: null }
    },
    blueprint: {
      uuid: 'b04aff42-d892-4ca9-991e-101c869a7caa',
      output_name: '10-Series Greatsword Cannon',
      is_available_by_default: true,
      game_version: '4.10.3-LIVE.13165946',
      dismantle_returns: [
        { name: 'Iron', quantity_scu: 0.32 },
        { name: 'Titanium', quantity_scu: 0.16 }
      ],
      summary_properties: [
        { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher' },
        { property_key: 'weapon_damage', label: 'Impact Force', better_when: 'higher' }
      ],
      aspects: [
        { key: 'FRAME', name: 'Frame', required_count: 1,
          input: { name: 'Iron', min_quality: 1, quantity_scu: 0.64 },
          modifiers: [
            { property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher',
              quality_range: { min: 0, max: 1000 },
              modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 },
              value_range_type: 'linear' }
          ] },
        { key: 'CYCLER', name: 'Cycler', required_count: 1,
          input: { name: 'Riccite', min_quality: 1, quantity_scu: 0.09 },
          modifiers: [
            { property_key: 'weapon_damage', label: 'Impact Force', better_when: 'higher',
              quality_range: { min: 0, max: 1000 },
              modifier_range: { at_min_quality: 0.95, at_max_quality: 1.05 },
              value_range_type: 'linear' }
          ] }
      ],
      unlocking_missions_grouped: [
        { label: 'Guaranteed', chance: 1, missions: [
          { title: '[TargetName] needs stomping', debug_name: 'HH_Pyro_RegionA_VLRT_Criminals_KillShip', reward_scope: 'Bounty Hunter', count: 1, web_url: null },
          { title: 'Crash some ships', debug_name: 'HH_Pyro_RegionA_M_2AsteroidBase_Criminals_MultiKillship', reward_scope: 'Assassination', count: 2, web_url: null }
        ] },
        { label: 'Vulnerability Assessment', chance: 0.4, missions: [
          { title: 'Hit a CFP Ship', debug_name: 'HB_CFP_HitShip', reward_scope: 'Bounty Hunter', count: 1, web_url: null }
        ] }
      ],
      web_url: 'https://api.star-citizen.wiki/blueprints/10-series-greatsword-cannon'
    },
    attribution: 'Data from star-citizen.wiki'
  };

  window.__STRATA_CALLS = [];
  const orig = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    if (cmd === 'strata_status') return { enabled: true, key_set: true, key_ref: 'default', tier_default: 4, scwiki_ready: true };
    if (cmd === 'strata_items') { window.__STRATA_CALLS.push({ cmd }); return ITEMS; }
    if (cmd === 'strata_craft_cost') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      return a.item === 'item-1' ? CRAFT_BP : CRAFT_NOBP;
    }
    if (cmd === 'scwiki_craft_detail') {
      window.__STRATA_CALLS.push({ cmd, args: a });
      // M8-K5-Fix-Probe: simulierter Backend-Fehler (z. B. API-Ausfall).
      if (window.__SCWIKI_FAIL) throw new Error('SCWiki offline (probe)');
      return a.itemName === 'Greatsword Cannon' ? SCWIKI_DETAIL : null;
    }
    return orig(cmd, args);
  };
  window.craftDetailOpen = () => !!document.querySelector('.crafting .cd-title');
  window.openCraft = (label) => {
    const b = [...document.querySelectorAll('.crafting .crow-main')].find((x) => x.textContent.includes(label));
    if (!b) return false;
    b.click();
    return true;
  };
  window.propTexts = () => [...document.querySelectorAll('.crafting .cprop')].map((x) => x.textContent.replace(/\s+/g, ' ').trim());
  window.descOpen = () => {
    const d = document.querySelector('.crafting .cd-desc');
    if (!d) return null;
    return { present: true, open: d.open, text: d.querySelector('p')?.textContent.trim() ?? '' };
  };
  window.statText = () => {
    const s = document.querySelector('.crafting .cstats');
    return s ? s.textContent.replace(/\s+/g, ' ').trim() : null;
  };
  window.dismText = () => {
    const s = document.querySelector('.crafting .cdism');
    return s ? s.textContent.replace(/\s+/g, ' ').trim() : null;
  };
  window.missionState = () => {
    const m = document.querySelector('.crafting .cmissions');
    if (!m) return null;
    const groups = [...m.querySelectorAll('.cmg')].map((g) => ({
      label: g.querySelector('.cmg-label')?.textContent.trim(),
      guaranteed: g.className.includes('is-guaranteed'),
      chance: g.querySelector('.cmg-chance')?.textContent.trim() ?? null,
      missions: [...g.querySelectorAll('.cm-mission')].map((x) => ({
        title: x.querySelector('.cm-title')?.textContent.trim(),
        debug: x.querySelector('.cm-debug')?.textContent.trim() ?? null,
        scope: [...x.querySelectorAll('.cbadge')].map((b) => b.textContent.trim())
      }))
    }));
    return groups;
  };
  window.qsPresent = () => !!document.querySelector('.crafting .qs');
  window.attrText = () => document.querySelector('.crafting .cattr')?.textContent.trim() ?? '';
  // M8-K5-Fix: Fehler-Hinweis + Retry (kein stummes Schlucken mehr).
  window.scwikiErrState = () => {
    const e = document.querySelector('.crafting .cscwiki-err');
    return {
      present: !!e,
      text: e?.textContent.replace(/\s+/g, ' ').trim() ?? null,
      retry: !!document.querySelector('.crafting .cscwiki-retry')
    };
  };
  window.scwikiCalls = () => window.__STRATA_CALLS.filter((c) => c.cmd === 'scwiki_craft_detail').length;
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
        "--remote-debugging-port=9234", "--user-data-dir=/tmp/cdp-m8k5-craft-detail",
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

        # ---- Gate 1: ohne SC-Wiki-Match bleiben die Welle-2-Blöcke weg ----
        await js("window.pressKey('F6')")
        await wait_for("document.querySelector('.crafting')", 8)
        await js("openCraft('Plain Cannon')")
        await wait_for("craftDetailOpen()", 8)
        check("1a Detail offen (Plain Cannon)", await js("craftDetailOpen()") is True)
        await wait_for("window.__STRATA_CALLS.some((c) => c.cmd === 'scwiki_craft_detail')", 6)
        check("1b scwiki_craft_detail aufgerufen (Cross-Ref-Pfad)",
              await js("window.__STRATA_CALLS.some((c) => c.cmd === 'scwiki_craft_detail')") is True)
        check("1c Keine Kopf-Props ohne Match", await js("propTexts()") == [])
        check("1d Keine Beschreibung ohne Match", await js("descOpen()") is None)
        check("1e Kein Stat-Block ohne Match", await js("statText()") is None)
        check("1f Kein Missions-Block ohne Match", await js("missionState()") is None)
        check("1g Kein dismantle-Block ohne Match", await js("dismText()") is None)
        check("1h Kein QualityStats ohne Match", await js("qsPresent()") is False)
        await shot("k5-1-ohne-match.png")
        await js("window.pressKey('Escape')")

        # ---- Gate 2-7: MIT SC-Wiki-Match (Greatsword) ----
        await js("openCraft('Greatsword Cannon')")
        await wait_for("craftDetailOpen()", 8)
        await wait_for("propTexts().length > 0", 8)
        props = await js("propTexts()")
        print(f"    props: {props}")
        check("2a Kopf-Props vorhanden", isinstance(props, list) and len(props) >= 4)
        joined = " | ".join(props)
        check("2b Size S2", "S2" in joined, joined)
        check("2c Grade A", "A" in joined)
        check("2d Mass 192 kg", "192 kg" in joined)
        check("2e Muni S1", "Munition" in joined or "S1" in joined)
        check("2f KEINE Leerzeilen-Props (class null degradiert)", "—" not in joined, joined)
        check("2g Standard-Blaupause-Badge", "Standard blueprint" in joined, joined)

        desc = await js("descOpen()")
        check("3a Beschreibung vorhanden (klappbar)", isinstance(desc, dict) and desc["present"] is True)
        check("3b Beschreibung deutscher Text (de_EN)", "KnightBridge Arms" in (desc or {}).get("text", ""))
        await js("document.querySelector('.crafting .cd-desc summary')?.click()")
        await asyncio.sleep(0.2)
        desc_open = await js("descOpen()")
        check("3c Beschreibung öffnet per Klick", (desc_open or {}).get("open") is True)

        stats = await js("statText()")
        check("4a Stat-Block vorhanden", isinstance(stats, str) and "Weapon stats" in stats, stats)
        check("4b Damage/shot 73", "73" in stats)
        check("4c RPM 250", "250" in stats)
        check("4d Range 2703 m", "2703 m" in stats)
        check("4e Capacity 492", "492" in stats)
        check("4f damages-Liste mit dmg-Typen", "Kinetic: 40" in stats and "Distortion: 33" in stats, stats)
        check("4g Ammunition-Stats (m/s)", "1100 m/s" in stats, stats)

        dism = await js("dismText()")
        check("5a dismantle_returns unter Materialliste", isinstance(dism, str) and "Dismantle returns" in dism, dism or "")
        check("5b Iron 0.32 SCU", "0.32" in dism, dism)
        check("5c Titanium 0.16 SCU", "0.16" in dism, dism)

        missions = await js("missionState()")
        check("6a Missions-Block vorhanden", isinstance(missions, list) and len(missions) == 2, missions)
        g0 = missions[0]
        check("6b Guaranteed-Gruppe hervorgehoben", g0["label"] == "Guaranteed" and g0["guaranteed"] is True, g0)
        check("6c Guaranteed ohne %-Angabe (chance=1)", g0["chance"] is None, g0)
        check("6d Chance-Gruppe mit %-Angabe", missions[1]["chance"] == "40 %", missions[1])
        check("6e reward_scope als Badge", "Bounty Hunter" in g0["missions"][0]["scope"], g0)
        check("6f Debug-Name kompakt", g0["missions"][0]["debug"] == "HH_Pyro_RegionA_VLRT_Criminals_KillShip", g0)
        check("6g count ×2", "×2" in g0["missions"][1]["title"], g0["missions"][1])
        # Missions-Sektion in den Viewport scrollen (liegt unter dem Planner).
        await js("document.querySelector('.crafting .cmissions')?.scrollIntoView({ block: 'center' })")
        await asyncio.sleep(0.3)
        await shot("k5-2-mit-match.png")

        # ---- Gate 7: K6-Verdrahtung (aspects aus dem Blueprint) ----
        check("7a QualityStats erscheint via SC-Wiki-Aspekte", await js("qsPresent()") is True)
        check("7b Attribution SC-Wiki zusätzlich",
              "Data from star-citizen.wiki" in await js("attrText()"), await js("attrText()"))
        # Distinkte Sektions-Aufnahmen (Review-Finding 2): Kopf/Stats, dann Planner/QS.
        await js("document.querySelector('.crafting .cstats')?.scrollIntoView({ block: 'start' })")
        await asyncio.sleep(0.3)
        await shot("k5-3-qualitystats.png")

        # ---- Gate 8 (Review-Blocker): scwiki_craft_detail-Err NICHT stumm ----
        await js("window.__SCWIKI_FAIL = true")
        await js("window.pressKey('Escape')")
        await js("openCraft('Greatsword Cannon')")
        await wait_for("craftDetailOpen()", 8)
        await wait_for("scwikiErrState().present", 8)
        err = await js("scwikiErrState()")
        check("8a Fehler-Hinweis sichtbar (Backend-Err)", err["present"] is True, err)
        check("8b Hinweistext (i18n, nicht raw key)", "SC-Wiki data could not be loaded" in (err["text"] or ""), err)
        check("8c Retry-Button vorhanden", err["retry"] is True, err)
        await shot("k5-4-scwiki-error.png")
        n_before = await js("scwikiCalls()")
        await js("document.querySelector('.crafting .cscwiki-retry')?.click()")
        await asyncio.sleep(0.3)
        err2 = await js("scwikiErrState()")
        check("8d Retry bei weiterem Fehler bleibt im Hinweis", err2["present"] is True, err2)
        check("8e Retry feuert neuen Fetch", await js("scwikiCalls()") == n_before + 1)
        # Fehler wieder weg → Retry lädt die Daten und der Hinweis verschwindet.
        await js("window.__SCWIKI_FAIL = false")
        await js("document.querySelector('.crafting .cscwiki-retry')?.click()")
        await wait_for("propTexts().length > 0 && !scwikiErrState().present", 8)
        check("8f Retry nach Erholung: Daten da, Hinweis weg",
              await js("propTexts().length > 0") is True and (await js("scwikiErrState()"))["present"] is False)
        check("8g QualityStats nach Retry-Verdrahtung", await js("qsPresent()") is True)

    server.kill()
    proc.kill()
    print(f"\nPASS={PASS} FAIL={FAIL}")
    raise SystemExit(1 if FAIL else 0)


if __name__ == "__main__":
    asyncio.run(main())
