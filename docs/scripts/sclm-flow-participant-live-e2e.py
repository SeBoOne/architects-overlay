#!/usr/bin/env python3
"""Live-E2E für t_bf2374f7 (SF2): treibt den Teilnehmer-Flow-Request-Zyklus
gegen den LOKALEN SCLM-Container (Flask, isolierter DATA_DIR) — exakt die
Request-Formen, die der Flow/Screen-Actions generiert:

  1. GET  /api/session/{code}            (code_entry, X-SCLM-Code: join_code)
  2. POST /api/session/{sid}/participants {name}        → pcode (addme)
  3. POST /api/session/{sid}/wishes      {participant_id, loot_ids:[...],
        mode:"wish", pcode}              (wishes-Confirm-Action, Multi-Array)
  4. POST /api/session/{sid}/raffle      (Ziehung, admin_code — Ersteller)
  5. GET  /api/session/{sid}/raffle/result_for/{participant_id}  (results)
  6. PATCH /api/session/{sid}/raffle/{rid} {waived:true, pcode}   (Verzichten)
  7. ... nochmal Ziehung + PATCH {collected:true, pcode}          (Abholen)

Writes laufen NUR gegen den lokalen Container (isolierte DATA_DIR-Kopie).
"""
import json, os, subprocess, sys, time, urllib.request

PORT = int(os.environ.get("SCLM_PORT", "5177"))
BASE = f"http://127.0.0.1:{PORT}"
REPO = os.path.expanduser("~/Projects/sclm/repos/sclm")
DATA_DIR = "/tmp/sclm-e2e-data"

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
    subprocess.run(["rm", "-rf", DATA_DIR])
    env = dict(os.environ, DATA_DIR=DATA_DIR, PORT=str(PORT), SHOW_SECRET_ON_START="0")
    proc = subprocess.Popen(
        ["/tmp/sclm-venv/bin/python", "server.py", str(PORT)], cwd=REPO, env=env,
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

        # 1) Session erstellen (Ersteller-Seite, hier nur als Fixture)
        st, s = req("POST", "/api/session/create", {"name": "SF2-E2E", "max_wishes": 3})
        check("create-session", st == 200 and "join_code" in s, f"{st} {s}")
        sid, jc, ac = s["id"], s["join_code"], s["admin_code"]

        # Loot (Admin, Fixture für die Wünsche)
        st, l1 = req("POST", f"/api/session/{sid}/loot", {"name": "Cutlass Black", "qty": 1}, code=ac)
        st2, l2 = req("POST", f"/api/session/{sid}/loot", {"name": "Caterpillar", "qty": 1}, code=ac)
        check("loot-added", st == 200 and st2 == 200, f"{st} {st2}")

        # 2) code_entry: GET /api/session/{join_code} — Session öffnet
        st, sess = req("GET", f"/api/session/{jc}", code=jc)
        check("code-entry-fetches-session", st == 200 and sess.get("id") == sid and sess.get("max_wishes") == 3,
              f"{st} {str(sess)[:120]}")
        check("code-entry-lists-loot-and-participants",
              len(sess.get("loot", [])) == 2 and sess.get("participants") == [],
              f"loot={len(sess.get('loot', []))} parts={sess.get('participants')}")

        # 3) addme-Action: POST participants {name} → pcode im Response
        st, p = req("POST", f"/api/session/{sid}/participants", {"name": "Sebo"}, code=jc)
        check("addme-returns-pcode", st == 200 and p.get("pcode") and p.get("id"),
              f"{st} {p}")
        pid, pcode = p["id"], p["pcode"]

        # 4) wishes-Confirm-Action: loot_ids als ECHTES Array (Multi-Select)
        st, w = req("POST", f"/api/session/{sid}/wishes",
                    {"participant_id": pid, "loot_ids": [l1["id"], l2["id"]],
                     "mode": "wish", "pcode": pcode}, code=jc)
        check("wishes-post-with-loot-array", st == 200 and w.get("ok") is True, f"{st} {w}")
        # pcode-Claim: Participant (self-added) hat pcode — zweiter Save mit
        # falschem pcode muss abgelehnt werden (Eigentumsnachweis).
        st, w2 = req("POST", f"/api/session/{sid}/wishes",
                     {"participant_id": pid, "loot_ids": [l1["id"]], "mode": "wish",
                      "pcode": "WRONG"}, code=jc)
        check("wishes-rejects-wrong-pcode", st == 403, f"{st} {w2}")

        # 5) Ziehung (Ersteller, admin_code)
        st, r = req("POST", f"/api/session/{sid}/raffle", {}, code=ac)
        check("raffle-starts", st == 200, f"{st} {r}")
        st, sess2 = req("GET", f"/api/session/{jc}", code=jc)
        check("raffle-done-sqlite-int", sess2.get("raffle_done") in (1, True), f"{sess2.get('raffle_done')}")

        # 6) results: result_for → rid/won
        st, res = req("GET", f"/api/session/{sid}/raffle/result_for/{pid}", code=jc)
        rid = (res.get("result") or {}).get("id")
        check("results-result-for", st == 200 and rid and (res["result"].get("won")),
              f"{st} {str(res)[:160]}")

        # 7) Verzichten (waived) — PATCH mit pcode
        st, wv = req("PATCH", f"/api/session/{sid}/raffle/{rid}",
                     {"waived": True, "pcode": pcode}, code=jc)
        check("waive-patch", st == 200 and wv.get("ok") is True, f"{st} {wv}")
        st, res2 = req("GET", f"/api/session/{sid}/raffle/result_for/{pid}", code=jc)
        check("waive-visible-in-result", (res2.get("result") or {}).get("waived") in (1, True),
              str(res2)[:160])

        # 8) Abholen: zweite Session, gleicher Zyklus, collected statt waived
        st, s2 = req("POST", "/api/session/create", {"name": "SF2-E2E-2", "max_wishes": 2})
        sid2, jc2, ac2 = s2["id"], s2["join_code"], s2["admin_code"]
        st, l3 = req("POST", f"/api/session/{sid2}/loot",
                     {"name": "Freelancer", "qty": 1, "type": "vehicle", "grade": "C", "size": "M"}, code=ac2)
        st, p2 = req("POST", f"/api/session/{sid2}/participants", {"name": "Sebo"}, code=jc2)
        st, w3 = req("POST", f"/api/session/{sid2}/wishes",
                     {"participant_id": p2["id"], "loot_ids": [l3["id"]], "mode": "wish",
                      "pcode": p2["pcode"]}, code=jc2)
        st, _ = req("POST", f"/api/session/{sid2}/raffle", {}, code=ac2)
        st, res3 = req("GET", f"/api/session/{sid2}/raffle/result_for/{p2['id']}", code=jc2)
        rid2 = (res3.get("result") or {}).get("id")
        st, cl = req("PATCH", f"/api/session/{sid2}/raffle/{rid2}",
                     {"collected": True, "pcode": p2["pcode"]}, code=jc2)
        check("collect-patch", st == 200 and cl.get("ok") is True, f"{st} {cl}")
        st, res4 = req("GET", f"/api/session/{sid2}/raffle/result_for/{p2['id']}", code=jc2)
        check("collect-visible-in-result", (res4.get("result") or {}).get("collected") in (1, True),
              str(res4)[:160])

        # ---- SF2-R1-Fixes (Review t_bf2374f7 Runde 1) ------------------------
        # D3: admin-angelegter Teilnehmer hat pcode=null — wishes-POST ohne
        # pcode → 403; erster Save mit selbst gewähltem Claim-Code bindet (200).
        st, pa = req("POST", f"/api/session/{sid2}/participants", {"name": "AdminLegtAn"}, code=ac2)
        check("D3-admin-participant-pcode-null", st == 200 and pa.get("pcode") is None, f"{st} {pa}")
        st, w403 = req("POST", f"/api/session/{sid2}/wishes",
                       {"participant_id": pa["id"], "loot_ids": [l3["id"]], "mode": "wish"}, code=jc2)
        check("D3-select-path-wishes-without-pcode-403", st == 403, f"{st} {w403}")
        st, wclaim = req("POST", f"/api/session/{sid2}/wishes",
                         {"participant_id": pa["id"], "loot_ids": [l3["id"]], "mode": "wish",
                          "pcode": "CLAIM-SELBST"}, code=jc2)
        check("D3-claim-with-chosen-pcode-binds", st == 200, f"{st} {wclaim}")
        st, wwrong = req("POST", f"/api/session/{sid2}/wishes",
                         {"participant_id": pa["id"], "loot_ids": [], "mode": "wish",
                          "pcode": "FALSCH"}, code=jc2)
        check("D3-wrong-claim-rejected-after-bind", st == 403, f"{st} {wwrong}")
        # D2: raffle_done bleibt SQLite-Int 1 — Exit-Bedingung (code-Screen
        # `raffle_done equals true → results`) wertet serverseitig identisch aus.
        st, sess3 = req("GET", f"/api/session/{jc2}", code=jc2)
        check("D2-raffle-done-int-for-exit-cond", sess3.get("raffle_done") in (1, True),
              str(sess3.get("raffle_done")))
        # D4: Loot-Zeilen tragen type/grade/size (Chips-Datenquelle).
        check("D4-loot-rows-carry-type-grade-size",
              all(k in (sess3.get("loot") or [{}])[0] for k in ("type", "grade", "size")),
              str((sess3.get("loot") or [{}])[0]))
    finally:
        proc.terminate()
        try: proc.wait(timeout=5)
        except subprocess.TimeoutExpired: proc.kill()

    print(f"\n{len(PASS)} PASS / {len(FAIL)} FAIL")
    if FAIL:
        raise SystemExit("FAILED: " + ", ".join(FAIL))

if __name__ == "__main__":
    main()
