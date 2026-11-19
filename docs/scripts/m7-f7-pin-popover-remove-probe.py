#!/usr/bin/env python3
"""CDP-Probe für t_84008140 (M7-F7: Signatur-Pin-Popover — Einzelsignaturen
per Klick entfernen, modusunabhängig).

Belegt auf der echten Svelte-Oberfläche (Tauri-IPC gestubbt, Fixture-Pin):
  A Pfad 1 (primär, interaktives Overlay): Chips sind im interaktiven
     Spotlight-Kontext NATIVE Buttons (.sigs-chip-btn) — Klick entfernt DEN
     EINEN Entry (dedupe-Key oreId) über pin_update mit aktualisierter
     Payload; nach 2 Klicks sind 4 von 6 Einträgen übrig.
  B Pfad 2 (Popover): Kopf-Klick (unter DRAG_THRESHOLD) → Popover mit
     Signaturliste OBEN (Kürzel + Basis-Signatur, Rarity-Chip) und
     Standard-Controller UNTEN; ✕ pro Entry entfernt den EINEN Eintrag,
     Popover-Inhalt lebt (Refresh via PINS_CHANGED); letzter ✕ → pin_remove
     → Pin + Popover weg.
  C Modusunabhängigkeit: F2 → Market-Modus — Pin + Chip-Klick + Popover
     funktionieren identisch (PinBoxes rendert in allen Modi).
  D pins-only (Palette zu, ignore_cursor_events): Chips sind reine Anzeige
     (KEIN Button) — M4-E-Vertrag unangetastet; Entfernen läuft dort über
     das Pin-Fenster gar nicht (auch vorher nicht).
Screenshots nach docs/screens/m7-f7-pin-popover-remove/.
"""
import asyncio, base64, json, os, pathlib
import websockets, urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
STUB = (ROOT / "docs/scripts/smoke-stub.js").read_text()
OUT = pathlib.Path(os.environ.get("SHOT_DIR", str(ROOT / "docs/screens/m7-f7-pin-popover-remove")))
OUT.mkdir(parents=True, exist_ok=True)
PORT = 8797
URL = f"http://127.0.0.1:{PORT}/"
CDP_PORT = 9251

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail else ""))

ENTRIES = [
    {"oreId": "gold_raw", "label": "Gold", "value": 30, "rarity": "rare"},
    {"oreId": "quantainium_raw", "label": "Quantainium", "value": 90, "rarity": "legendary"},
    {"oreId": "laranite_raw", "label": "Laranite", "value": 55, "rarity": "epic"},
    {"oreId": "tungsten_raw", "label": "Tungsten", "value": 64, "rarity": "uncommon"},
    {"oreId": "beryl_raw", "label": "Beryl", "value": 12, "rarity": "common"},
    {"oreId": "bexalite_raw", "label": "Bexalite", "value": 41, "rarity": "uncommon"},
]

WRAP = r"""
(function () {
  window.__m7f7 = { calls: [], overlay: __OVERLAY__ };
  const row = (id, name, abbrev) => ({ idx: String(id), name: abbrev, sub: name, value: String(30 + id), side: null });
  // ---- Fake-Pins-Store (pin_update/pin_remove wie pins.rs) ----
  const S = window.__m7f7;
  S.pins = __SEED__;
  const copy = (p) => ({ ...p, rows: (p.rows || []).map((r) => ({ ...r })),
                         content: p.content ? JSON.parse(JSON.stringify(p.content)) : null });
  const orig = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    const a = args || {};
    S.calls.push({ cmd, args: a });
    if (cmd === 'overlay_get_state') return S.overlay;
    if (cmd === 'pins_state') {
      return { enabled: true, game_running: true, overlay_visible: S.overlay.visible,
               spotlight_screen: 'DP-3', pins: S.pins.filter((p) => true).map(copy) };
    }
    if (cmd === 'pin_update') {
      let hit = false;
      for (const p of S.pins) {
        if (p.pinned && p.view_id === a.view_id && p.box_index === a.box_index) {
          p.title = a.title; p.kpi = a.kpi ?? null; p.rows = a.rows || [];
          p.content = a.content ?? null; if (a.key != null) p.key = a.key;
          hit = true;
        }
      }
      if (hit) window.__pinsChanged();
      return hit;
    }
    if (cmd === 'pin_remove') {
      let hit = false;
      for (const p of S.pins) {
        if (p.pinned && p.view_id === a.view_id && p.screen === a.screen && p.box_index === a.box_index) { p.pinned = false; hit = true; }
      }
      if (hit) window.__pinsChanged();
      return hit;
    }
    if (/^(pin_move|pin_set_opacity|pin_unpin|pin_set_scale|pin_resize|pin_add)$/.test(cmd)) return true;
    return await orig(cmd, args);
  };
  // PINS_CHANGED_EVENT manuell dispatchen (Backend-emit-Äquivalent).
  window.__pinsChanged = function () {
    (globalThis.__evtListeners?.get('ao:pins-changed') || []).forEach((f) => f({ payload: true }));
  };
})();
"""

def wrap_src(overlay, seed_pins):
    return WRAP.replace("__OVERLAY__", json.dumps(overlay)).replace("__SEED__", json.dumps(seed_pins))

def seed_collect():
    return [{
        "view_id": "mining", "screen": "DP-3", "box_index": 9903, "opacity": 100,
        "pinned": True, "x": 24, "y": 120,
        "title": "SCAN-SIGNATUREN", "kpi": None,
        "rows": [{"idx": str(i), "name": e["label"][:2].capitalize(), "sub": e["label"],
                  "value": str(e["value"]), "side": None} for i, e in enumerate(ENTRIES)],
        "content": {"type": "scan-signatures", "entries": [dict(e) for e in ENTRIES]},
        "key": "scan-signatures",
    }]

async def main():
    server = await asyncio.create_subprocess_exec(
        "python3", "-m", "http.server", str(PORT), "--bind", "127.0.0.1",
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
        "--window-size=1920,1200", "--hide-scrollbars",
        f"--remote-debugging-port={CDP_PORT}", "--user-data-dir=/tmp/cdp-m7f7",
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
        raise RuntimeError("chromium CDP did not come up")

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

        CONSOLE = []
        async def recv_loop():
            async for raw in cdp:
                msg = json.loads(raw)
                if msg.get("method") == "Runtime.consoleAPICalled":
                    try:
                        CONSOLE.append(" ".join(str(a.get("value", a.get("description", a.get("type")))) for a in msg["params"].get("args", [])))
                    except Exception:
                        pass
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
                return "EXC:" + (r["exceptionDetails"].get("text") or "")
            return r.get("result", {}).get("value")

        async def shot(name, clip=None):
            params = {"format": "png"}
            if clip:
                params["clip"] = clip
            r = ok(await call("Page.captureScreenshot", params))
            data = base64.b64decode(r["data"])
            (OUT / name).write_bytes(data)
            print("  shot:", OUT / name, len(data), "bytes")

        async def key(k, vk):
            await call("Input.dispatchKeyEvent", {"type": "rawKeyDown", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await call("Input.dispatchKeyEvent", {"type": "keyUp", "key": k, "code": k,
                                                  "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk})
            await asyncio.sleep(0.5)

        VK = {"F2": 113, "F5": 116}
        await call("Page.enable")
        await call("Runtime.enable")

        PAL_OPEN = {"visible": True, "game_running": True, "click_through": False}
        PINS_ONLY = {"visible": False, "game_running": True, "click_through": True}
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": wrap_src(PAL_OPEN, seed_collect())})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(3)
        await key("F5", VK["F5"])
        await asyncio.sleep(1)

        # =========== A — Pfad 1: Chip-Direktklick (interaktiver Kontext) =====
        n = await js("document.querySelectorAll('main .pin').length")
        check("a1-collect-pin-rendered", n == 1, f"pins={n}")
        rows = await js("document.querySelectorAll('main .pin .sigs-row').length")
        check("a2-six-entries", rows == 6, f"rows={rows}")
        btns = await js("document.querySelectorAll('main .pin .sigs-chip-btn').length")
        check("a3-chips-are-native-buttons-interactive", btns == 6, f"buttons={btns}")
        curs = await js("getComputedStyle(document.querySelector('main .pin .sigs-chip-btn')).cursor")
        check("a4-chip-cursor-pointer", curs == "pointer", curs)

        await js("window.__m7f7.calls.length = 0;")
        await js("document.querySelectorAll('main .pin .sigs-chip-btn')[1].click()")
        await asyncio.sleep(0.6)
        upd = await js("JSON.stringify(window.__m7f7.calls.filter(function(c){return c.cmd==='pin_update';}).pop() || null)")
        upj = json.loads(upd) if upd and upd != "null" else None
        ids = [x["oreId"] for x in (upj["args"]["content"]["entries"] if upj else [])]
        check("a5-chip-click-removes-one-via-pin-update",
              upj is not None and "quantainium_raw" not in ids and len(ids) == 5, str(ids))
        await asyncio.sleep(0.6)
        rows = await js("document.querySelectorAll('main .pin .sigs-row').length")
        check("a6-live-refresh-5-rows", rows == 5, f"rows={rows}")
        await js("document.querySelectorAll('main .pin .sigs-chip-btn')[0].click()")
        await asyncio.sleep(0.8)
        upd2 = json.loads(await js("JSON.stringify(window.__m7f7.calls.filter(function(c){return c.cmd==='pin_update';}).pop() || null)"))
        ids2 = [x["oreId"] for x in (upd2["args"]["content"]["entries"] if upd2 else [])]
        check("a7-second-click-4-entries-left",
              upd2 is not None and len(ids2) == 4 and "gold_raw" not in ids2, str(ids2))
        check("a8-payload-contract-kept", upd2 is not None and upd2["args"]["content"]["type"] == "scan-signatures"
              and upd2["args"]["kpi"] is None and [r["idx"] for r in upd2["args"]["rows"]] == ["0", "1", "2", "3"],
              json.dumps(upd2)[:220])
        rows = await js("document.querySelectorAll('main .pin .sigs-row').length")
        check("a9-live-refresh-4-rows", rows == 4, f"rows={rows}")
        await shot("01-interactive-chip-click-4-left.png")

        # =========== B — Pfad 2: Popover (Signaturliste + Controller) =========
        # Kopf-Klick OHNE Drag-Bewegung (pointerdown + pointerup, 0 px Delta).
        await js("""(function(){
            var h=document.querySelector('main .pin .pin-head .pin-title');
            var r=h.getBoundingClientRect();
            var x=r.x+r.width/2, y=r.y+r.height/2;
            var ev=new PointerEvent('pointerdown',{bubbles:true,clientX:x,clientY:y,button:0});
            h.dispatchEvent(ev);
            window.dispatchEvent(new PointerEvent('pointerup',{bubbles:true,clientX:x,clientY:y,button:0}));
        })()""")
        await asyncio.sleep(0.6)
        pop = await js("""JSON.stringify((function(){
            var p=document.querySelector('main .pin-popover.has-sigs');
            if(!p) return null;
            return {entries: p.querySelectorAll('.sigs-entry').length,
                    chips: p.querySelectorAll('.se-chip').length,
                    xs: p.querySelectorAll('.se-x').length,
                    ctl: p.querySelectorAll('.pbtn-row .pbtn').length};
        })())""")
        pj = json.loads(pop) if pop and pop != "null" else None
        check("b1-popover-has-sigs-list", pj is not None and pj["entries"] == 4 and pj["chips"] == 4 and pj["xs"] == 4, pop)
        check("b2-controller-kept-below", pj is not None and pj["ctl"] >= 8, pop)
        await shot("02-popover-list-plus-controller.png")
        # ✕ 1: Popover-Inhalt lebt (nächster Entry wählbar)
        await js("document.querySelector('main .pin-popover .se-x').click()")
        await asyncio.sleep(0.8)
        pop = json.loads(await js("""JSON.stringify((function(){
            var p=document.querySelector('main .pin-popover.has-sigs');
            return p ? {entries: p.querySelectorAll('.sigs-entry').length} : null;
        })())""") or "null")
        check("b3-popover-live-after-x", pop is not None and pop["entries"] == 3, str(pop))
        for _ in range(3):  # restliche 3 Entries → letzter ✕ entfernt den Pin
            await js("document.querySelectorAll('main .pin-popover .se-x')[0].click()")
            await asyncio.sleep(0.9)
        rem = await js("JSON.stringify(window.__m7f7.calls.filter(function(c){return c.cmd==='pin_remove';}).pop() || null)")
        remj = json.loads(rem) if rem and rem != "null" else None
        check("b4-last-x-removes-pin", remj is not None and remj["args"]["view_id"] == "mining"
              and remj["args"]["box_index"] == 9903, rem[:200])
        gone = await js("!document.querySelector('main .pin') && !document.querySelector('main .pin-popover')")
        check("b5-pin-and-popover-gone", bool(gone))
        await shot("03-empty-list-pin-removed.png")

        # =========== C — Modusunabhängigkeit: Market-Modus (F2) ==============
        # frischer Seed (der Pin aus A/B wurde vollständig entfernt — auch das
        # ist ein Beleg: leere Liste → Pin weg). Neues Document = Stub neu.
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": wrap_src(PAL_OPEN, seed_collect())})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(3)
        await key("F5", VK["F5"])
        await asyncio.sleep(1)
        await key("F2", VK["F2"])
        await asyncio.sleep(1.2)
        n = await js("document.querySelectorAll('main .pin').length")
        mining_open = await js("!!document.querySelector('.mining')")
        check("c1-pin-rendered-in-market", n == 1 and not mining_open, f"pins={n} mining={mining_open}")
        # Popover im Market-Modus öffnen + Entry entfernen
        await js("""(function(){
            var h=document.querySelector('main .pin .pin-head .pin-title');
            var r=h.getBoundingClientRect();
            var x=r.x+r.width/2, y=r.y+r.height/2;
            h.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,clientX:x,clientY:y,button:0}));
            window.dispatchEvent(new PointerEvent('pointerup',{bubbles:true,clientX:x,clientY:y,button:0}));
        })()""")
        await asyncio.sleep(0.6)
        pop = await js("!!document.querySelector('main .pin-popover.has-sigs')")
        check("c2-popover-in-market-mode", bool(pop))
        await shot("04-popover-market-mode.png")
        await js("window.__m7f7.calls.length = 0;")
        dbg_chip = await js("""JSON.stringify((function(){
            var b=document.querySelectorAll('main .pin .sigs-chip-btn');
            return {n: b.length, tag: b[0] ? b[0].tagName : null,
                    dis: b[0] ? b[0].disabled : null, ev: b[0] ? !!b[0].onclick || b[0].getAttribute('onclick') : null};
        })())""")
        print("  debug chips-in-market:", dbg_chip)
        await js("document.querySelector('main .pin .sigs-chip-btn').click()")
        await asyncio.sleep(0.7)
        print("  debug console:", [c for c in CONSOLE[-12:]])
        print("  debug calls:", await js("JSON.stringify(window.__m7f7.calls.map(function(c){return c.cmd;}))"))
        # frischer Seed (6 Entries) → der Klick entfernt EINEN Entry über
        # pin_update; Pin bleibt mit 5 Einträgen bestehen.
        chip_upd = await js("window.__m7f7.calls.some(function(c){return c.cmd==='pin_update';})")
        rows_after = await js("document.querySelectorAll('main .pin .sigs-row').length")
        check("c3-chip-click-works-in-market", bool(chip_upd) and rows_after == 5,
              f"upd={chip_upd} rows={rows_after}")

        # =========== D — pins-only: reine Anzeige (M4-E-Vertrag) =============
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": STUB})
        await call("Page.addScriptToEvaluateOnNewDocument", {"source": wrap_src(PINS_ONLY, seed_collect())})
        await call("Page.navigate", {"url": URL})
        await asyncio.sleep(3)
        await key("F5", VK["F5"])
        await asyncio.sleep(1)
        btns = await js("document.querySelectorAll('main .pin .sigs-chip-btn').length")
        rows = await js("document.querySelectorAll('main .pin .sigs-row').length")
        check("d1-pins-only-chips-plain-display", btns == 0 and rows == 6, f"buttons={btns} rows={rows}")
        await shot("05-pins-only-display-only.png")

    server.terminate(); proc.terminate()
    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        print("FAILED:", ", ".join(FAIL))
        raise SystemExit(1)

asyncio.run(main())
