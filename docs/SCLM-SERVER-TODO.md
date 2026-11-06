# SCLM Server — Offene Aufgaben (API-Haertung)

> **ERLEDIGT 2026-11-06:** Alle fünf Punkte sind auf dem Server umgesetzt (Haertung 3A/3B/3C
> + 4A im Repo SeBoOne/sclm, main 2b118db, live deployt). Live-Gegenprobe Omar-Rev1
> 2026-11-06: numeric size -> 400 'size muss eine Zeichenkette sein'; qty 1e20/20000 ->
> 400 'muss zwischen 1 und 10000 sein'; qty 1.5 -> 400 Ganzzahl; Re-Wunsch auf
> Verkauf-Item -> 400 'Gegenstand ist fuer Verkauf markiert' (loot_ids im Body);
> participants[].wish_count/no_interest_count vorhanden; Sessions-Liste ohne Codes;
> GET /api/public/session/<sid> 200; sid im GET-Pfad -> 404 mit Hinweis-JSON.
> Overlay-seitige Konsequenzen: docs/SCLM-FLOW-V2.md §25 (Scan-Zweit-Endpunkt,
> Deploy-Reihenfolge Binary vor Manifest-Re-Import). Overlay-Haertungen (Coercion,
> qty-Klemme) bleiben als Doppel-Sicherung bestehen. Datei bleibt als
> Vertrags-Dokumentation stehen.

Stand 2026-11-05. Alle Befunde sind live auf https://sclm.site reproduziert (Wegwerf-Sessions,
mittlerweile geloescht). Produziert durch Diagnose-Serien des Architects-Overlay-Projekts
(Omar/Sebo). Kontext je Fundstelle: docs/SCLM-FLOW-V2.md §19 §20 §21 §23.

## 1. Zahlen in Text-Feldern paniken mit nacktem 500-HTML (wichtigster Punkt)

Der Loot-Dedupe-Key `name|type|grade|size` wird auf Serverseite als Saite gebaut. Bekommt
ein Text-Feld einen JSON-Zahlenwert, crash der Handler mit nacktem HTTP 500 (HTML-Seite,
kein JSON, kein Servertext) — fuer Clients ununterscheidbar von einem Server-Neustart.

Repro:
```sh
SID=<sid einer Test-Session>
ADMIN=<admin_code>
# 500-HTML:
curl -s -o /dev/null -w "%{http_code}\n" -X POST "https://sclm.site/api/session/$SID/loot" \
  -H "Content-Type: application/json" -H "X-SCLM-Code: $ADMIN" \
  -d '{"name":"FR-86","type":"MIL","grade":"A","size":3}'
# 200 (string):
curl -s -o /dev/null -w "%{http_code}\n" -X POST "https://sclm.site/api/session/$SID/loot" \
  -H "Content-Type: application/json" -H "X-SCLM-Code: $ADMIN" \
  -d '{"name":"FR-86","type":"MIL","grade":"A","size":"3"}'
```
Erwartetes Verhalten: Typfehler ablehnen mit `400 {"error": "..."}` (klares Feld-Fehlerwort),
oder Felder tolerant als String coercen. Betroffene Endpunkte: POST/PATCH loot, commodities,
participants (dort `name`; auch dort werden Namen concatiniert).

## 2. Grenzenlose Zahlen werden gespeichert und vergiften spaetere Listen

EXTREME-FALL (live bestaetigt): ein qty-Wert jenseits int64 wurde mit 200 GESPEICHERT;
danach lieferte `GET /api/sessions` fuer ALLE Clients nur noch nacktes 500-HTML — die
Session-Liste war unbenutzbar, bis die betroffene Session geloescht wurde. Ebenso wurde
`qty: -1e20` und `scu: 1e29` mit 200 akzeptiert.

Erwartetes Verhalten:
- qty / sale_revenue / scu / delta: numeric Validierung VOR dem Speichern
  (Integer-Bereich, sensible Obergrenze, plausible Vorzeichen) → sonst `400`.
- Die Serialization von `GET /api/sessions` und `GET /api/session/<code>` darf nicht
  crashen, wenn bereits gespeicherte Ausreisser-Werte in der DB liegen (robuste
  Serialisierung, Not-garbage-collect).
- Bestehende DB auf Ausreisser-Values pruefen (historisch Gespeichertes kann jede
  Listen-Abfrage aller Clients lahmlegen).

## 3. Code nur im GET-Pfad — andere Verben brauchen sid (Doku explizit machen)

Verifiziert 2026-11-05: `GET /api/session/<code>` akzeptiert join_code UND admin_code.
`GET /api/session/<sid>` (sid im Pfad) → JSON `{"error":"Code ungültig"}`. PATCH mit
join_code im Pfad → 403. Die API.md-ENDpunkt-Tabelle mischt `<sid>`/`<code>`-Platzhalter —
Tools die der Doku folgen, probieren code in Write-Verben und scheitern still.

Erwartetes Verhalten: In API.md je Endpunkt explizit markieren, ob sid oder code erwartet
wird; Fehlerantworten durchgehend als klares JSON (`{"error": "sid erwartet"}` statt
 nackter 403/404).

## 4. (Optional, hilfreich) wish_count je Teilnehmer in der Session-API

`participants[].wishes` ist eine Liste nackter loot_id-Saiten. Client-UIs die eine Anzahl
brauchen, müssen die Liste selbst zaehlen; verschachtelte Mapping-Ausdruecke der Art
`wishes[].id` crashen Client-seitig (dem Overlay ist genau das passiert, docs §23).
Ein additives Feld `"wish_count": <int>` am Participant-Objekt (und analog
`"assigned_count"` wenn das Ziehungsergebnis Zuweisungen enthaelt) waere ein robuster
Kontrakt. Nur additiv — bestehende Felder nicht umbauen.

## 5. Doku-Kleinkram

- `size` in loot-Antworten: die API liefert mal Saiten ("3") — Requests mit Zahlen
  siehe Punkt 1. Eindeutige Typ-Zusage je Feld in API.md (int vs string) wuerde
  Client-Implementatoren vor Punkt-1-Ueberraschungen schuetzen.
- Fehlerantworten einheitlich als JSON (auch 404/405): derzeit gemischt HTML/JSON;
  Clients die JSON erwarten, zeigen dann "Ungueltiges JSON" statt eines Hinweises.

## 5. Wuensche auf Verkauf-Items ablehnen (statt stumm zu speichern)

Verifiziert 2026-11-05 (Omar-Diag9): Server zieht `for_sale`-Items korrekt aus
der Ziehung raus und loescht bestehende Wuenche automatisch beim
Verkauf-Markieren — beides gut. Aber `POST …/wishes` mit einem `for_sale`-
loot_id wird mit 200 `{"ok":true}` angenommen und gespeichert: tote, nie
gewinnbare Wuensche in der DB (Teilnehmer sehen sie als ihre Wuensche).

Erwartetes Verhalten: `POST …/wishes` lehnt `for_sale`-loot_ids mit klarem
400 ab (`{"error": "Gegenstand ist fuer Verkauf markiert"}`) oder filtert sie
mit Hinweis im Response. Overlay sperrt die Auswahl bereits clientseitig.
