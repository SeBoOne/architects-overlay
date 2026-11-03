#!/usr/bin/env python3
"""Live-E2E für t_cce3a5ea (SCLM v2 Karte C): Teilnehmer-Flow v2.

Treibt die Teilnehmer-Kette gegen den LOKALEN SCLM-Container (Flask,
isolierte DATA_DIR) — exakt in den Request-Formen, die der v2-Flow
(`open`-Block + FlowRenderer-Aktionen) erzeugt. KEIN code_entry-Screen
mehr: der Einstieg ist der `connector_flow_open`-Zyklus (Suchzeile oder
Öffnen-Button mit gespeichertem join_code), nachgestellt als dieselbe
HTTP-Sequenz:

  1. Fixture: Session + Loot anlegen (Ersteller-Angabe, nur Untergrund)
  2. Flow-Open: GET /api/session/<join_code> (X-SCLM-Code: join_code)
     → Join-Sicht entfernt Secrets (admin_code/pcode nicht im Body)
  3. Auto-Join: POST participants {name} → {id, pcode (servergeneriert)}
  4. Duplikat (ci): POST gleicher Name → 409 {error, id} → Claim-id-Weg
  5. result_for VOR Ziehung: 200 {"result": null} → Client-null-Pfad
  6. POST wishes {participant_id, loot_ids:[echtes Array], mode, pcode}
     → ersetzt vollständig; max_wishes-Grenze; pcode NIE im Header
  7. Claim-Pfad: admin-angelegter Teilnehmer (pcode NULL) → clientseitig
     generierter pcode (≥16 url-safe) bindet beim ersten wishes; danach
     verlangt (403 bei fremdem pcode)
  8. wait-Exit: Poll GET session bis raffle_done (exit-Bedingung then results)
  9. results: result_for jetzt gesetzt; PATCH {collected:true, pcode}
     one-way — collected:false-Versuch lässt Flag auf 1; waived analog
     für den Claim-Teilnehmer

Voraussetzung: lokale SCLM-Instanz-Quelle (~/Projects/sclm/repos/sclm) und
ein Python mit Flask, z. B.:
  uv venv /tmp/sclm-venv && uv pip install --python /tmp/sclm-venv/bin/python flask
Start: python3 docs/scripts/sclm-flow-participant-live-e2e.py
Writes laufen NUR gegen den lokalen Container (isolierte DATA_DIR-Kopie).
"""
import json, os, secrets, subprocess, sys, time, urllib.request

PORT = int(os.environ.get("SCLM_PORT", "5177"))
BASE = f"http://127.0.0.1:{PORT}"
REPO = os.path.expanduser("~/Projects/sclm/repos/sclm")
DATA_DIR = os.environ.get("SCLM_E2E_DATA", "/tmp/sclm-e2e-data-c")
PY = os.environ.get("SCLM_PY", "/tmp/sclm-venv/bin/python")

PASS, FAIL = [], []
def check(name, ok, detail=""):
    (PASS if ok else FAIL).append(name)
    print(("  PASS " if ok else "  FAIL ") + name + ((" — " + detail) if detail and not ok else ""))

def req(method, path, body=None, code=None):
    r = urllib.request.Request(BASE + path, method=method)
    if code: r.add_header("X-SCLM-Code", code)
    data = json.dumps(body).encode() if body is not None else None
    if data: r.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(r, data=data, timeout=10) as resp:
            return resp.status, json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        try: return e.code, json.loads(e.read().decode())
        except Exception: return e.code, {}

def main():
    if not os.path.exists(PY):
        print(f"FEHLER: Python mit Flask nicht gefunden: {PY}\n"
              "  uv venv /tmp/sclm-venv && uv pip install --python /tmp/sclm-venv/bin/python flask")
        return 2
    subprocess.run(["rm", "-rf", DATA_DIR])
    env = dict(os.environ, DATA_DIR=DATA_DIR, PORT=str(PORT), SHOW_SECRET_ON_START="0")
    proc = subprocess.Popen(
        [PY, "server.py", str(PORT)], cwd=REPO, env=env,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(60):
            try:
                with urllib.request.urlopen(BASE + "/api/stats", timeout=1) as r:
                    if r.status == 200: break
            except Exception: pass
            time.sleep(0.25)
        else:
            raise RuntimeError("SCLM server did not come up")

        NAME = "Flow V2 Tester"

        # 1) Fixture-Session (Ersteller-Views sind v2 out-of-scope, nur Untergrund)
        st, s = req("POST", "/api/session/create", {"name": "C-v2-E2E", "max_wishes": 3})
        check("fixture-session-created", st == 200 and "join_code" in s and "admin_code" in s, f"{st} {s}")
        sid, jc, ac = s["id"], s["join_code"], s["admin_code"]

        # Loot als Wunsch-Untergrund (4 Items, max_wishes=3)
        okl = True
        for lname in ("Loot Alpha", "Loot Beta", "Loot Gamma", "Loot Extra"):
            st, _l = req("POST", f"/api/session/{sid}/loot", {"name": lname, "qty": 1}, code=ac)
            okl = okl and st == 200
        check("loot-added", okl)
        # GET /api/session/<code> akzeptiert NUR join_code/admin_code (Pfad)
        st, d = req("GET", f"/api/session/{ac}", code=ac)
        loot_ids = [l["id"] for l in d.get("loot", [])]
        check("loot-ids-readable", len(loot_ids) >= 4, f"{len(loot_ids)}")

        # 2) Flow-Open (Suchzeilen-Code): Session via join_code lasten
        st, d = req("GET", f"/api/session/{jc}", code=jc)
        check("open-via-join-code", st == 200 and d.get("id") == sid, f"{st}")
        check("join-view-hides-secrets",
              d.get("admin_code") is None
              and all(p.get("pcode") is None for p in d.get("participants", [])))

        # 3) Auto-Join: Name fehlt in Teilnehmerliste → POST → {id, pcode}
        st, j = req("POST", f"/api/session/{sid}/participants", {"name": NAME}, code=jc)
        check("auto-join-returns-server-pcode",
              st == 200 and bool(j.get("pcode")) and bool(j.get("id")), f"{st} {j}")
        pid, pcode = j.get("id"), j.get("pcode")

        # 4) Duplikat (case-insensitive) → 409 mit `id` → Claim-Pfad (Keystore-pcode)
        st, dup = req("POST", f"/api/session/{sid}/participants", {"name": NAME.lower()}, code=jc)
        check("duplicate-409-carries-id", st == 409 and dup.get("id") == pid, f"{st} {dup}")

        # 5) results-Vorweg: result_for VOR Ziehung → 200 {result: null} (null-safe)
        st, r0 = req("GET", f"/api/session/{sid}/raffle/result_for/{pid}", code=jc)
        check("result-null-before-raffle", st == 200 and r0.get("result") is None, f"{st} {r0}")

        # 6) wishes-Confirm (Multi-Array, pcode im Body, mode nach raffle_mode)
        st, _w = req("POST", f"/api/session/{sid}/wishes",
                     {"participant_id": pid, "loot_ids": loot_ids[:2], "mode": "wish", "pcode": pcode},
                     code=jc)
        check("wishes-accepts-real-array", st == 200, f"{st} {_w}")
        st, d = req("GET", f"/api/session/{jc}", code=jc)
        mine = next((p for p in d.get("participants", []) if p.get("id") == pid), {})
        check("wishes-stored", sorted(mine.get("wishes") or []) == sorted(loot_ids[:2]))

        # Ersetzt vollständig (idempotenter Replace-Vertrag): nur noch ein loot_ids-Eintrag
        st, _w = req("POST", f"/api/session/{sid}/wishes",
                     {"participant_id": pid, "loot_ids": [loot_ids[0]], "mode": "wish", "pcode": pcode},
                     code=jc)
        st, d = req("GET", f"/api/session/{jc}", code=jc)
        mine = next((p for p in d.get("participants", []) if p.get("id") == pid), {})
        check("wishes-replace-fully", mine.get("wishes") == [loot_ids[0]], f"{mine.get('wishes')}")

        # max_wishes-Grenze (Flow begrenzt Auswahl; Server erzwingt trotzdem)
        st, mw = req("POST", f"/api/session/{sid}/wishes",
                     {"participant_id": pid, "loot_ids": loot_ids[:4], "mode": "wish", "pcode": pcode},
                     code=jc)
        check("max-wishes-guard-400", st == 400, f"{st} {mw}")

        # pcode-Regel (Spec Abschnitt 3): NIE im Header (wird ignoriert), nur im Body
        st, _w = req("POST", f"/api/session/{sid}/wishes",
                     {"participant_id": pid, "loot_ids": [loot_ids[0]], "mode": "wish"},
                     code=pcode)  # pcode als X-SCLM-Code: kein Session-Code → 403
        check("pcode-not-valid-as-session-header", st == 403, f"{st}")
        st, _w = req("POST", f"/api/session/{sid}/wishes",
                     {"participant_id": pid, "loot_ids": [loot_ids[0]], "mode": "wish"},
                     code=jc)  # Header ok, aber Body ohne pcode → 403
        check("wishes-without-body-pcode-403", st == 403, f"{st}")

        # 7) Claim-Pfad: admin-angelegter Teilnehmer (pcode NULL) → client-generierter
        #    pcode (≥16 url-safe) bindet beim ersten wishes, danach verlangt
        claim_pcode = secrets.token_urlsafe(18)
        st, po = req("POST", f"/api/session/{sid}/participants", {"name": "Orphan Guest"}, code=ac)
        check("admin-participant-pcode-null", st == 200 and po.get("pcode") is None, f"{st} {po}")
        st, _ow = req("POST", f"/api/session/{sid}/wishes",
                      {"participant_id": po["id"], "loot_ids": [loot_ids[1]], "mode": "wish",
                       "pcode": claim_pcode},
                      code=jc)
        check("claim-binds-client-pcode", st == 200, f"{st} {_ow}")
        st, _ow = req("POST", f"/api/session/{sid}/wishes",
                      {"participant_id": po["id"], "loot_ids": [loot_ids[1]], "mode": "wish",
                       "pcode": "fremder-code-1234567890abcdef"},
                      code=jc)
        check("bound-pcode-demanded-after-claim", st == 403, f"{st}")

        # 8) wait→results: Ziehung (Ersteller-Code) → Poll bis raffle_done (wait-exit)
        st, rf = req("POST", f"/api/session/{sid}/raffle", {}, code=ac)
        check("raffle-executes", st == 200, f"{st} {rf}")
        done = False
        for _ in range(10):
            st, dp = req("GET", f"/api/session/{jc}", code=jc)
            if dp.get("raffle_done"):
                done = True
                break
            time.sleep(0.2)
        check("wait-poll-exits-on-raffle-done", done)

        # 9) results: result_for jetzt mit Datensatz
        st, r1 = req("GET", f"/api/session/{sid}/raffle/result_for/{pid}", code=jc)
        check("result-present-after-raffle", st == 200 and r1.get("result") is not None, f"{st}")
        rid = (r1.get("result") or {}).get("id")

        # 10) Abgeholt-Button: PATCH {collected:true, pcode} → one-way
        st, _pc = req("PATCH", f"/api/session/{sid}/raffle/{rid}",
                      {"collected": True, "pcode": pcode}, code=jc)
        check("patch-collected-with-pcode", st == 200, f"{st} {_pc}")
        st, r2 = req("GET", f"/api/session/{sid}/raffle/result_for/{pid}", code=jc)
        check("collected-flag-set", bool((r2.get("result") or {}).get("collected")))
        st, _pu = req("PATCH", f"/api/session/{sid}/raffle/{rid}",
                      {"collected": False, "pcode": pcode}, code=jc)
        st, r3 = req("GET", f"/api/session/{sid}/raffle/result_for/{pid}", code=jc)
        check("collected-one-way-stays-set", bool((r3.get("result") or {}).get("collected")))

        # Verzichten analog (Claim-Teilnehmer, pcode aus dem Keystore-Pendant)
        st, rr = req("GET", f"/api/session/{sid}/raffle/result_for/{po['id']}", code=jc)
        rid_orphan = (rr.get("result") or {}).get("id")
        st, _wa = req("PATCH", f"/api/session/{sid}/raffle/{rid_orphan}",
                      {"waived": True, "pcode": claim_pcode}, code=jc)
        check("waive-works-for-claimed-participant", st == 200, f"{st} {_wa}")

        print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
        if FAIL:
            raise SystemExit("FAILED: " + ", ".join(FAIL))
    finally:
        proc.terminate()
        try: proc.wait(timeout=5)
        except subprocess.TimeoutExpired: proc.kill()

if __name__ == "__main__":
    sys.exit(main() or 0)
