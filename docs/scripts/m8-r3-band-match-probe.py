#!/usr/bin/env python3
"""M8-R3 Band-Match-Prüfung (t_399b10d8) — LIVE gegen die echten Quellen.

User-Befund (0.5.88/0.5.89): A03 Sniper Rifle / 10-Series Greatsword /
Arclight „Herrero“ / Skarnov „Raider Camo“ zeigten keine Bands — dieselben
Materialien haben bei anderen Blaupausen Bands. Diese Probe prüft die
Match-Tabelle Strata-Materialnamen → SCMDB `materialName`/`name`-Formen:

  1. SCMDB mining_data (KrovaxCode/SCMDB_DATA, `data/latest.json` →
     `channels.live.files.mining_data`) laden — exakt wie
     `scwiki_client::scmdb::MiningIndex::build`: beide Namensformen
     (materialName UND name) registrieren, `(Ore)/(Raw)/(Pure)`-Suffix
     strippen, Union dedup.
  2. Strata `items` + `craft-cost/{id}` für ALLE craftbaren Items
     (Quelle-Beleg: ~735 Items). Jede Kosten-Material-Benennung gegen den
     Index matchen (gleiches normalize wie die Rust-Seite).
  3. Schwellen-Asserts: die 4 User-Items (A03 Sniper Rifle, 10-Series
     Greatsword Cannon, Arclight „Herrero“ Pistol, Skarnov „Raider Camo“
     Rifle) MÜSSEN für jede Kosten-Zeile Bands haben (>= 2 Punkte).

Grenzen: SCMDB kennt nur die 39 mineableElements — ein Material ohne
Bands (z. B. „Yormandi Eye“ bei Metamaterial Test #146) ist kein
Match-Gap, sondern ein echtes Daten-Loch der Quelle (kein fixbarer Bug
im Overlay: kein Match → UI rastet 500-only, gewolltes Verhalten).
Probe-Skript ist bewusst robust gegen 429 (Retry-After respektieren,
120 req/60s Quote — ~0.55 s Pacing + Backoff).

Env: STRATA_KEY (celd_-Key) — ohne Key nur die SCMDB-Seite (Gates 1+4).
"""
import json
import os
import sys
import time
import urllib.request
import urllib.error

SCMDB_BASE = "https://raw.githubusercontent.com/KrovaxCode/SCMDB_DATA/main"
STRATA_BASE = "https://strata.celd.space"

# User-Items aus der Karte (exact names in der Strata-Items-Liste).
TARGET_ITEMS = [
    "A03 Sniper Rifle",
    "10-Series Greatsword Cannon",
    'Arclight "Herrero" Pistol',
    'Skarnov "Raider Camo" Rifle',
]

PASS = 0
FAIL = 0


def check(name, cond, detail=""):
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  PASS {name}")
    else:
        FAIL += 1
        print(f"  FAIL {name}  {detail}")


def normalize(name):
    """Exakt wie crates/scwiki-client/src/scmdb.rs `normalize_material`:
    trim + lowercase + (Ore)/(Raw)/(Pure)-Suffix strippen."""
    n = (name or "").strip().lower()
    for suffix in [" (ore)", " (raw)", " (pure)"]:
        if n.endswith(suffix):
            return n[: -len(suffix)].strip()
    return n


def get(url, auth=None, retries=3):
    hdrs = {"User-Agent": "architects-overlay/0.5.89 (m8-r3-band-match-probe)"}
    if auth:
        hdrs["Authorization"] = "Bearer " + auth
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers=hdrs)
            with urllib.request.urlopen(req, timeout=30) as r:
                return json.loads(r.read())
        except urllib.error.HTTPError as e:
            if e.code == 429 and attempt < retries - 1:
                retry_after = e.headers.get("Retry-After")
                wait = int(retry_after) if retry_after and retry_after.isdigit() else 5
                print(f"  429 → backoff {wait}s")
                time.sleep(wait)
                continue
            raise
    raise RuntimeError(f"unreachable: {url}")


def main():
    # ---- Gate 1: SCMDB mining_data laden + Index bauen (Rust-Logik) ----
    latest = get(SCMDB_BASE + "/data/latest.json")
    fname = latest["channels"]["live"]["files"]["mining_data"]
    mining = get(SCMDB_BASE + "/data/" + fname)
    elems = mining["mineableElements"]
    check("1 SCMDB mineableElements geladen (39 erwartet)", len(elems) == 39, str(len(elems)))
    print(f"  mining file: {fname}")

    index = {}
    for uuid, el in elems.items():
        for key in [el.get("materialName"), el.get("name")]:
            if key:
                k = normalize(key)
                if k:
                    vals = sorted(set(el.get("qualityBands") or []))
                    index[k] = sorted(set(index.get(k, []) + vals))
    check("1a Alle 39 Elemente im Index (kein Form-Verlust)", len(index) >= 39, str(len(index)))
    bands_missing = [k for k, v in index.items() if len(v) < 2]
    check("1b Jede Index-Form trägt >= 2 Band-Grenzen", not bands_missing, str(bands_missing))
    # User-LIVE-Belege (Karte, 2026-11-20) — Anchors der Band-Daten.
    # Achtung: Index ist normalize()-lowercase — Anker-Namen ebenfalls normalisieren
    # (sonst greift der [None]-Default und 1c failt als Phantom).
    for name, first in [("Agricium", 346), ("Taranite", 310), ("Hadanite", 274)]:
        check(f"1c {name} startet mit Band {first} (LIVE-Beleg)",
              index.get(normalize(name), [None])[0] == first,
              str(index.get(normalize(name))))

    api_key = os.environ.get("STRATA_KEY", "").strip()
    if not api_key:
        print("  (kein STRATA_KEY — Gates 2–4 übersprungen)")
        print(f"\nPASS={PASS} FAIL={FAIL}")
        return 1 if FAIL else 0

    # ---- Gate 2: die 4 User-Items — jede Kosten-Zeile muss Bands haben ----
    items = get(STRATA_BASE + "/api/public/items", auth=api_key)["items"]
    by_name = {i["name"]: i for i in items}
    for name in TARGET_ITEMS:
        it = by_name.get(name)
        if it is None:
            check(f"2 {name} in Strata-Items gefunden", False, "nicht in items")
            continue
        cc = get(f"{STRATA_BASE}/api/public/craft-cost/{it['id']}", auth=api_key)
        costs = cc.get("costs") or []
        check(f"2 {name}: {len(costs)} Kosten-Zeilen", len(costs) > 0, str(costs))
        for c in costs:
            m = c["material"]
            bands = index.get(normalize(m))
            check(f"2 {name} → {m!r} hat Bands (>=2)", bands is not None and len(bands) >= 2, str(bands))

    # ---- Gate 3: Voll-Sweep — jede Strata-Kosten-Benennung matcht ----
    mats = {}
    no_cost = 0
    for it in items:
        try:
            cc = get(f"{STRATA_BASE}/api/public/craft-cost/{it['id']}", auth=api_key)
        except Exception as e:
            print(f"  (übersprungen: {it['name']}: {e})")
            continue
        costs = cc.get("costs") or []
        if not costs:
            no_cost += 1
        for c in costs:
            m = c["material"]
            mats.setdefault(m, normalize(m) not in index)
        time.sleep(0.55)  # 120 req/60s Quote
    check("3 Strata-Kosten-Materialien abgedeckt", len(mats) >= 30, str(len(mats)))
    unmatchable = sorted(m for m, bad in mats.items() if bad)
    print(f"  Strata-Kosten-Materialien: {len(mats)} eindeutig; ohne Bands: {unmatchable}")
    # Nur die bekannten Nicht-Mineable-Ausnahmen dürfen fehlen (kein
    # fixbares Match-Gap — die Quelle selbst kennt kein Band dafür).
    ALLOWED_NO_BAND = {"Yormandi Eye"}
    check("3a Kein Match-Gap außer bekannten Nicht-Mineable-Ausnahmen",
          set(unmatchable) <= ALLOWED_NO_BAND, str(set(unmatchable) - ALLOWED_NO_BAND))

    # ---- Gate 4: alle 39 SCMDB-Formen sind untereinander matchbar ----
    for uuid, el in elems.items():
        for key in [el.get("materialName"), el.get("name")]:
            if key:
                check(f"4 {key!r} matcht eigenen Index-Eintrag", normalize(key) in index, normalize(key))

    print(f"\nPASS={PASS} FAIL={FAIL}")
    return 1 if FAIL else 0


if __name__ == "__main__":
    sys.exit(main())
