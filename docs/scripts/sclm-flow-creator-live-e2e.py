#!/usr/bin/env python3
"""Live-E2E für t_5b7d59a5 (SF3): treibt den Ersteller-Flow-Request-Zyklus
gegen den LOKALEN SCLM-Container (Flask, isolierter DATA_DIR) — exakt die
Request-Formen, die der Creator-Flow (create/open/manage/settings/participants/
wishes_for/loot/commodities/results/result) generiert:

  1. POST   /api/session/create {name, max_wishes}      → id, admin_code, join_code
  2. GET    /api/session/{ac}     (open/manage — Admin sieht Codes)
  3. PATCH  /api/session/{sid}/settings {max_wishes}    (settings)
  4. POST   /api/session/{sid}/participants {name}      (add, admin)
  5. GET    /api/participants/search?q=                 (Autocomplete-Quelle)
  6. PATCH  /api/session/{sid}/participants/{pid} {name}(rename)
  7. DELETE /api/session/{sid}/participants/{pid}       (remove)
  8. POST   /api/session/{sid}/loot {name,qty,type,grade,size} (add_loot)
  9. GET    /api/items/search?q=                        (Autocomplete + Auto-Fill)
 10. PATCH  /api/session/{sid}/loot/{lid}/qty {delta}   (Menge)
 11. PATCH  /api/session/{sid}/loot/{lid}/sale {for_sale:true} (zum Verkauf)
 12. POST   /api/session/{sid}/commodities {name,scu}; GET commodities/search; PATCH scu
 13. POST   /api/session/{sid}/wishes {participant_id, loot_ids, mode:"wish"} (Admin, ohne pcode)
 14. POST   /api/session/{sid}/raffle (Ziehung, admin_code) → results[] im Session-Payload
 15. GET    /api/session/{sid}/raffle/result_for/{pid}; PATCH /raffle/{rid} {collected|waived}
 16. Negativ: settings ohne Admin-Code → 403

Writes laufen NUR gegen den lokalen Container (isolierte DATA_DIR-Kopie).
"""
import json, os, subprocess, sys, time, urllib.request, urllib.error

PORT = int(os.environ.get("SCLM_PORT", "5178"))
BASE = f"http://127.0.0.1:{PORT}"
REPO = os.path.expanduser("~/Projects/sclm/repos/sclm")
DATA_DIR = "/tmp/sclm-e2e-data-sf3"

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

        # 1) create (Flow-Screen `create`: mapping sid/code/admin_code/join_code + Pins)
        st, s = req("POST", "/api/session/create", {"name": "SF3-E2E", "max_wishes": 3})
        check("create-session", st == 200 and all(k in s for k in ("id", "admin_code", "join_code")), f"{st} {s}")
        sid, ac, jc = s["id"], s["admin_code"], s["join_code"]

        # 2) open/manage: Admin-Code öffnet Session, sieht Codes + is_admin
        st, s = req("GET", f"/api/session/{ac}", code=ac)
        check("open-with-admin-code", st == 200 and s.get("is_admin") is True, f"{st}")
        check("codes-visible-for-admin", s.get("admin_code") == ac and s.get("join_code") == jc)

        # 3) settings: max_wishes ändern (Flow-Screen `settings`)
        st, r = req("PATCH", f"/api/session/{sid}/settings", {"max_wishes": 4}, code=ac)
        check("settings-max-wishes", st == 200 and r.get("ok") is True, f"{st} {r}")
        _, s = req("GET", f"/api/session/{ac}", code=ac)
        check("settings-applied", s.get("max_wishes") == 4, str(s.get("max_wishes")))

        # 4) participants: hinzufügen (Admin → pcode null)
        st, p1 = req("POST", f"/api/session/{sid}/participants", {"name": "Alpha"}, code=ac)
        check("participant-add", st == 200 and p1.get("pcode", "x") is None, f"{st} {p1}")
        st, p2 = req("POST", f"/api/session/{sid}/participants", {"name": "Beta"}, code=ac)
        check("participant-add-2", st == 200, f"{st}")

        # 5) Namens-Autocomplete-Quelle
        st, hits = req("GET", "/api/participants/search?q=Alp")
        check("participants-search-endpoint", st == 200 and isinstance(hits, list), f"{st}")

        # 6) rename
        st, r = req("PATCH", f"/api/session/{sid}/participants/{p1['id']}", {"name": "Alpha2"}, code=ac)
        check("participant-rename", st == 200 and r.get("ok") is True, f"{st} {r}")

        # 7) remove
        st, r = req("DELETE", f"/api/session/{sid}/participants/{p2['id']}", code=ac)
        check("participant-remove", st == 200 and r.get("ok") is True, f"{st} {r}")
        _, s = req("GET", f"/api/session/{ac}", code=ac)
        names = [p["name"] for p in s["participants"]]
        check("participant-list-after", names == ["Alpha2"], str(names))

        # 8) loot: eintragen (add_loot: name/qty/type/grade/size)
        st, l1 = req("POST", f"/api/session/{sid}/loot",
                     {"name": "FS-55 Cannon", "qty": 1, "type": "weapon", "grade": "C", "size": "S2"}, code=ac)
        check("loot-add", st == 200 and l1.get("id"), f"{st} {l1}")
        st, l2 = req("POST", f"/api/session/{sid}/loot", {"name": "Rust Society Helmet", "qty": 1}, code=ac)
        check("loot-add-plain", st == 200, f"{st}")

        # 9) items/search: Autocomplete-Quelle mit type/grade/size (Auto-Fill)
        st, hits = req("GET", "/api/items/search?q=FS-55")
        row = next((h for h in hits if h.get("name") == "FS-55 Cannon"), None)
        check("items-search-autofill-source",
              st == 200 and row is not None and row.get("type") == "weapon"
              and row.get("grade") == "C" and row.get("size") == "S2", f"{st} {hits}")

        # 10) Menge (Delta)
        st, r = req("PATCH", f"/api/session/{sid}/loot/{l1['id']}/qty", {"delta": 1}, code=ac)
        _, s = req("GET", f"/api/session/{ac}", code=ac)
        qty = next(l["qty"] for l in s["loot"] if l["id"] == l1["id"])
        check("loot-qty-delta", st == 200 and qty == 2, f"{st} qty={qty}")

        # 11) zum Verkauf markieren (Waren-Reiter-Interaktion) + Wünsche werden entfernt
        st, w = req("POST", f"/api/session/{sid}/wishes",
                    {"participant_id": p1["id"], "loot_ids": [l1["id"]], "mode": "wish"}, code=ac)
        check("wishes-admin-set", st == 200 and w.get("ok") is True, f"{st} {w}")
        st, r = req("PATCH", f"/api/session/{sid}/loot/{l1['id']}/sale", {"for_sale": True}, code=ac)
        _, s = req("GET", f"/api/session/{ac}", code=ac)
        loot1 = next(l for l in s["loot"] if l["id"] == l1["id"])
        check("loot-for-sale", st == 200 and loot1.get("for_sale") == 1, f"{st} {loot1.get('for_sale')}")
        alpha = next(p for p in s["participants"] if p["id"] == p1["id"])
        check("for-sale-clears-wishes", alpha.get("wishes") == [], str(alpha.get("wishes")))
        st, r = req("PATCH", f"/api/session/{sid}/loot/{l1['id']}/sale",
                    {"for_sale": False, "sale_revenue": 1250.5}, code=ac)
        _, s = req("GET", f"/api/session/{ac}", code=ac)
        loot1 = next(l for l in s["loot"] if l["id"] == l1["id"])
        check("sale-unmark+revenue", st == 200 and loot1.get("for_sale") == 0 and loot1.get("sale_revenue") == 1250.5, f"{st}")

        # 12) Waren (Commodities)
        st, c = req("POST", f"/api/session/{sid}/commodities", {"name": "Titanium", "scu": 5}, code=ac)
        check("commodity-add", st == 200 and c.get("id"), f"{st} {c}")
        st, hits = req("GET", "/api/commodities/search?q=Tita")
        check("commodities-search-endpoint", st == 200 and isinstance(hits, list), f"{st}")
        st, r = req("PATCH", f"/api/session/{sid}/commodities/{c['id']}", {"scu": 7}, code=ac)
        _, s = req("GET", f"/api/session/{ac}", code=ac)
        check("commodity-scu-patch", st == 200 and s["commodities"][0]["scu"] == 7, f"{st}")

        # 13) Wünsche beidseitig (Admin setzt für Teilnehmer, ohne pcode)
        st, w = req("POST", f"/api/session/{sid}/wishes",
                    {"participant_id": p1["id"], "loot_ids": [l1["id"], l2["id"]], "mode": "wish"}, code=ac)
        _, s = req("GET", f"/api/session/{ac}", code=ac)
        alpha = next(p for p in s["participants"] if p["id"] == p1["id"])
        check("wishes-admin-replace", st == 200 and sorted(alpha.get("wishes", [])) == sorted([l1["id"], l2["id"]]), f"{st} {alpha.get('wishes')}")
        check("wish-count-visible", next(l for l in s["loot"] if l["id"] == l1["id"]).get("wish_count") == 1)

        # 14) Ziehung starten (admin_code) → results[] im Session-Payload
        st, r = req("POST", f"/api/session/{sid}/raffle", {}, code=ac)
        check("raffle-start", st == 200 and r.get("ok") is True, f"{st} {r}")
        _, s = req("GET", f"/api/session/{ac}", code=ac)
        check("raffle-done", s.get("raffle_done") == 1)
        rows = s.get("results", [])
        check("results-list-all-participants",
              len(rows) == 1 and rows[0]["participant_name"] == "Alpha2" and rows[0].get("won"), str(rows))

        # 15) Ergebnis-Detail + abgeholt/verzicht (Admin-PATCH ohne pcode)
        rid = rows[0]["id"]
        st, res = req("GET", f"/api/session/{sid}/raffle/result_for/{p1['id']}", code=ac)
        check("result-for", st == 200 and res.get("result", {}).get("id") == rid, f"{st}")
        st, r = req("PATCH", f"/api/session/{sid}/raffle/{rid}", {"collected": True}, code=ac)
        check("result-collect", st == 200 and r.get("ok") is True, f"{st} {r}")
        st, r = req("PATCH", f"/api/session/{sid}/raffle/{rid}", {"waived": True}, code=ac)
        _, s = req("GET", f"/api/session/{ac}", code=ac)
        row = next(x for x in s["results"] if x["id"] == rid)
        check("result-waive", st == 200 and row.get("waived") == 1 and row.get("collected") == 1, f"{st} {row}")

        # 16) Negativ: Admin-Write ohne Code → 403
        st, r = req("PATCH", f"/api/session/{sid}/settings", {"max_wishes": 9})
        check("settings-no-code-403", st == 403, f"{st}")
        st, r = req("DELETE", f"/api/session/{sid}/loot/{l1['id']}", code=jc)
        check("loot-remove-join-code-403", st == 403, f"{st}")

        print(f"\nSF3 Creator Live-E2E: {len(PASS)} PASS / {len(FAIL)} FAIL")
        if FAIL:
            print("FAILED: " + ", ".join(FAIL)); sys.exit(1)
    finally:
        proc.terminate()
        try: proc.wait(timeout=5)
        except Exception: proc.kill()

if __name__ == "__main__":
    main()
