# SCLM-Connector v2 — Teilnehmer-Flow (Spec & Entscheidungen)

**Status:** SPEC 2026-11-03, umgesetzt wird in Karten A→B→C (Board `architects-overlay`).
**Scope:** NUR Teilnehmer-Funktionen. Ersteller-/Admin-Funktionen bleiben als Manifest-Views bestehen, werden aber NICHT erweitert (Sebo-Beschluss 2026-11-03: "für den Anfang reine Teilnehmerfunktionen").

## 1. Anforderungen (Sebo, 2026-11-03 — kanonisch)

| # | Requirement |
|---|---|
| 1 | SCLM-Aufruf erfolgt im SCLM-Mode ausschließlich über die Suchzeile: dort wird ein Session-Code (join_code) eingegeben → öffnet die Verlosung. KEINE anderen Code-Eingabe-Screens mehr. |
| 2 | SCLM-Benutzername wird in den Overlay-EINSTELLUNGEN gespeichert: generische Plugin-Einstellungsgruppe (Manifest deklariert Settings-Felder; gilt auch für künftige Plugins). NICHT als Reiter im Mode. |
| 3 | Hauptansicht des SCLM-Modes = Liste aller Verlosungen, in denen der gespeicherte Benutzername in der Teilnehmerliste steht. |
| 4 | Listenaussehen wie Trade-Routen-/Suchergebnis-Listen (NICHT Tabelle). Angezeigte Infos: Eventname + Status (ausstehend / bereits verlost). |
| 5 | Listeneinträge sind nicht interaktionsfähig — AUẞER Verlosungen, deren Teilnehmer-Code (join_code) bereits zuvor eingegeben wurde: dafür ist `join_code (+sid, participant_id, pcode)` gespeichert → Eintrag bekommt "Öffnen"-Button. |
| 6 | Beim Öffnen läuft der Beitritt automatisch: fehlt der gespeicherte Name in der Teilnehmerliste → POST participants; existiert er (409 mit `id`) → Claim-Pfad. pcode bei Bedarf clientseitig generieren (≥16 Zeichen, url-safe), dauerhaft speichern und wiederverwenden. |
| 7 | Nach Wunsch-Auswahl automatisch auf die Warteseite (solange raffle_done=false); von dort automatisch zur persönlichen Gewinnübersicht mit Buttons Abholbestätigung / Verzichtserklärung. |
| 8 | Auf der Warteseite (raffle_done=false) gibt es einen Button "Wünsche ändern" → zurück zur Wunschliste, danach zurück zur Warteseite. |
| 9 | ALLES als Flow: kein manuelles Tab-/Reiter-Hin-und-her zwischen Warten- und Gewinn-Ansicht. Der Ablauf folgt seiner Reihenfolge. |
| 10 | Optische Tab-Regel (Sebo persönlich wichtig): Was wie ein Reiter aussieht, muss wie die bestehenden Chip-Reiter aussehen (`class="chip" class:is-on`, role="tab" — Muster `market.tab_shop`/`tab_trades` in `src/routes/+page.svelte` ~6040-6140). "Pill"-Buttons, die wie Tabs wirken, sind zu unterlassen. Flow-Schritte sind KEINE Tabs. |

## 2. Entscheidungen (Clarify 2026-11-03, Sebo)

| Punkt | Entscheidung |
|---|---|
| Username-Speicherort | Plugin-Einstellungsgruppe in den Overlay-Einstellungen (generisch, Manifest deklariert Felder) |
| Default-Name | Dedizierter SCLM-Name, LEER startend (kein UEX-Pre-fill) |
| Suchzeile im SCLM-Mode | NUR Code-Eingabe zum Öffnen (kein Listen-Filter) |
| Name-Match | Exakter Name, Groß-/Kleinschreibung egal (clientseitiger Vergleich) |
| Scan-Tiefe "Meine Verlosungen" | Alle Sessions mit `raffle_done=false` + die letzten 20 erledigten (aus `/api/sessions`, neueste zuerst) |

## 3. API-Fakten (aus `~/Projects/sclm/repos/sclm/docs/API.md`, verifiziert 2026-11-03 — verbindlich)

- Auth per Header: `X-SCLM-Code` (admin_code/join_code, case-insensitive). `pcode` NUR im JSON-Body (case-sensitive; Header wird ignoriert → 403).
- Overlay-HTTP läuft über reqwest (Rust) → CORS der API irrelevant.
- `GET /api/sessions` (ohne Auth, neueste zuerst): `{id, name, created, join_code, raffle_done, participant_count, loot_count, …}` — join_code wird öffentlich ausgeliefert (bewusst).
- `GET /api/session/<code>` (Pfad akzeptiert join_code ODER admin_code): volles Session-Objekt inkl. `participants[].{id,name,pcode|null,added_by,wishes}`, `loot[]`, `results[]`, `raffle_mode`, `max_wishes`, `locked`, `raffle_done`. Mit join_code: `pcode`/`admin_code` werden ENTFERNT.
- `POST …/participants {name}` mit join_code: legt an, Antwort `{id, pcode}` (pcode servergeneriert!). Duplikat (case-insensitive) → `409 {"error":"Name bereits vorhanden","id":"…"}` → vorhandene ID für Claim-Pfad nutzen.
- Claim herrenloser Teilnehmer (vom Admin angelegt, pcode null): erster `POST …/wishes` mit selbst erzeugtem `pcode` (≥16 Zeichen) bindet den Datensatz dauerhaft an diesen Client. Danach MUSS derselbe pcode mitgesendet werden (403 sonst).
- `POST …/wishes {participant_id, loot_ids:[…], pcode, mode:"wish"|"no_interest"}`: ERSETZT vollständig (idempotent bei gleichem Payload). `loot_ids` muss echtes JSON-Array sein. `mode` entsprechend Session-`raffle_mode` senden (API rechnet: `no_interest` wenn Lootmenge < Teilnehmerzahl). max_wishes-Grenze (400).
- `GET …/raffle/result_for/<pid>`: 200 mit `{"result": null}` solange keine Ziehung → Client muss `null` abfangen, nicht nur Status.
- `PATCH …/raffle/<rid> {collected:true, pcode}` bzw. `{waived:true, pcode}`: Teilnehmer darf nur SETZEN (one-way), Claim eines herrenlosen Ergebnisses funktioniert auch hier.
- Polling-Empfehlung API: laufendes Event 15–30 s, nach Ziehung 60–120 s. Kein Burst < 5 s. Rate-Limit primär auf raffle-POST (20/60 s), Lesungen unlimitiert.
- Kein Push/WebSocket → Flow-Transitions laufen über Poll des Manifest-`exit`-Mechanismus (bereits im FlowSchema: `exit {on, op, value, then}` + `poll_seconds` — siehe `wait`-Screen im aktuellen Manifest).

## 4. Aktueller Stand (Ist-Zustand, Basis main)

- Manifest: `examples/sclm.json` (id `sclm`, placement `kind:"own"`, hotkey_slot true) mit Views (`sessions` table, `session_detail`, Forms inkl. `join_session`, `set_wishes`, `raffle_mark`) und `flows`: `participant` (Screens: `code` → `participants` → `wishes` → `wait` → `results`) und `creator` (create/manage/…/c_results).
- Infrastruktur: `src-tauri/src/connector.rs` (reqwest, `StoredConnector` persistiert Manifest-JSON, Commands `connector_*`), `src/lib/connector.ts`, `src/lib/ConnectorViews.svelte` (685 Z.), `src/lib/FlowRenderer.svelte` (897 Z. — Screen-Ablauf, pins, actions, autocomplete).
- Settings-Plugin-Abschnitt existiert (Hinzufügen/Löschen/Aktivieren; Manifest-Auth-Felder erscheinen bei Deklaration). Screenshot: `docs/screenshots/connector-ui/04-plugins.png`.
- E2E-Probes existieren: `docs/scripts/sclm-flow-participant-live-e2e.py`, `sclm-flow-creator-live-e2e.py` (an lokale Instanz anpassen).
-Connector-Gesamtplanung: `docs/research/connector-system.md`.

### Was sich gegenüber dem aktuellen participant-Flow ändert
- Einstieg ist NICHT mehr ein `code_entry`-Screen im Flow, sondern die SUCHZEILE im SCLM-Mode (Code → Öffnen).
- Die `code`- und `participants`-Screens des participant-Flows entfallen als manuelle Schritte (Beitritt/Claim automatisiert beim Öffnen).
- `wishes`-Screen bleibt (Auswahl), `wait` bleibt (+ "Wünsche ändern"-Button), `results` bleibt (collect/waive) — Kette wird erzwungen linear.
- NEU: Landansicht des SCLM-Modes = "Meine Verlosungen"-Liste (vor Flow-Auswahl), gespeicherte Session-Codes决定 Öffnen-Button.

## 5. Neue Architektur-Bausteine

### 5.1 Manifest-Schema-Erweiterung `settings` (Karte A)
```jsonc
"settings_fields": [
  { "key": "sclm_username", "label": {"de": "SCLM-Benutzername", "en": "SCLM username"},
    "type": "text", "required": false, "secret": false }
]
```
- Rust: Validator + Store (Werte pro Connector persistiert, analog auth-Feld-Mechanik), Command zum Lesen/Setzen, Werte in Flow-/Fetch-Kontext (`{sclm_username}`-Placeholder wie bei params).
- UI: Einstellungen → Plugin-Abschnitt: je aktivem Connector mit `settings_fields` eine Gruppe mit den Feldern.
- i18n de/en.

### 5.2 Session-Keystore (Karte A)
Pro Connector lokal persistiert (neben StoredConnector, eigene JSON-Datei z. B. `connector-state/sclm.json`):
```jsonc
{ "sessions": [ { "sid": "9k2m4x7q", "join_code": "ABC123", "name": "Jumptown Raid #4",
    "participant_id": "p8n24rt", "pcode": "…", "result_id": "r3t4…", "added": 1730668102 } ] }
```
- Geschrieben bei: Code-Eingabe über Suchzeile (join_code+sid), automatischem Join/Claim (participant_id+pcode), erster Ergebnis-Sicht (result_id).
- Read API für UI + Flow-Pins. "Öffnen"-Button genau dann, wenn Eintrag mit `sid` im Keystore liegt.

### 5.3 "Meine Verlosungen"-Scan (Karte B)
- `GET /api/sessions` → filtere: alle `raffle_done=false` + die letzten 20 mit `raffle_done=true` (Liste ist schon neueste-zuerst).
- Pro Kandidat `GET /api/session/<join_code>` (join_code liegt in der Listenantwort), Match `participants[].name` case-insensitive gegen `sclm_username`.
- Cache im Rust-Connector (Poll 300 s analog bestehender Views; Detail-Fetches parallel, max. Kandidatenzahl begrenzt). Leerer Username → Liste zeigt Hinweis "Benutzername in Einstellungen setzen".
- Anzeige: Eventname + Status-Pill ("ausstehend" / "verlost"). Stil: Chip-/Ergebnislisten wie Trade-Routen (kein table).

### 5.4 Teilnehmer-Flow v2 (Karte C)
Öffnen (Suchzeile-Code ODER Öffnen-Button mit gespeichertem join_code):
1. AUTO-JOIN: Session laden → existiert mein Name (ci)? → `participant_id` merken. Sonst `POST participants` → `{id, pcode}` speichern. `409` → `id` aus Fehlerantwort → Claim-Pfad. pcode-Regel: Server-pcode (Self-Reg) oder selbst-generierter Claim-pcode — einmal festlegen, dauerhaft speichern, immer mitnehmen.
2. AUTO-TRANSITION: `raffle_done=true` direkt → Ergebnis-Ansicht (result_for, null-safe). Sonst → Wunschliste (multi-select, max max_wishes, `mode` nach raffle_mode) → Wünsche bestätigen (ersetzend, idempotent) → `wait`-Screen (poll 15–30 s, exit auf raffle_done) mit Button "Wünsche ändern" (→ Wunschliste, danach zurück zu wait) → automatisch `results` mit Buttons "Abgeholt"/"Verzichten" (one-way; UI deaktiviert gesetzte Flags).
3. Keine manuellen Tabs zwischen wait/results (Requirement 9). Tab-Optik nur Chip-Muster (Requirement 10).

## 6. Karten-Breakdown & Reihenfolge

1. **A — Settings-Felder + Keystore** (Grundlage: Schema, Rust-Store, Settings-UI, Commands, i18n, Tests)
2. **B — "Meine Verlosungen"-Landliste** (Scan + Styling + Öffnen-Button; hängt an A: Username + Keystore)
3. **C — Teilnehmer-Flow v2** (Auto-Join/Claim, wait/results-Flow, Suchzeilen-Einstieg; hängt an A+B)

Jeweils: Builder auf eigenem Branch, Reviewer-Review, Omar merged --no-ff. Kein Deploy zwischen den Karten; gemeinsamer Release nach C (VM-/Live-Gegentest gegen lokale SCLM-Instanz via `docs/scripts/sclm-flow-participant-live-e2e.py`-Muster).

## 7. Out of scope (v2)
- Ersteller-/Admin-Verbesserungen (bestehende Manifest-Views bleiben unangetastet)
- Mehrere SCLM-Instanzen pro Overlay (base_url bleibt pro Manifest)
- pcode-Sync über Geräte hinweg (Key export/import später, wenn Sebo es will)

## 8. Merge-Status 2026-11-04 (main, Omar)

- A+B+Härtung bereits in main (76bf179, bebc158, bd860e4); Karte C (Participant-Flow v2)
  folgt mit diesem Merge.
- Frontend-Grundlage ist durchgängig **C**: Karte Bs provisorische Scan-Landliste wurde
  durch C's Flow-Listen ersetzt; die Rust-Seite von B (scan_command, Poll-Cache) bleibt
  als Backend erhalten und gehärtet (poll_seconds-Deckel, code_field-Sanitiserung,
  sid-Dedup — aus t_3cfc7842).
- Tabs 'Warten auf Ziehung' / 'Mein Gewinn' sind entfallen (Sebo): ein Flow, kein
  manueller Reiter-Wechsel; Hotkey-Dialog-Subtabs bleiben unberührt.
- Nach Merge + Gates (fmt, clippy --workspace --all-targets, cargo test --workspace,
  pnpm check) folgt Release 0.5.43 nur mit Sebo (Tag + MSI-Build + Doku-Check);
  danach VM-Gegentest mit der Testerin (SAC-Rechner).

## 9. FlowId-Hotfix + Manifest-Audit (2026-11-04, main cc92243)

- **Hotfix:** `connector_flow_open`-Aufrufe (Suchleiste + Open-Button) übergaben
  `flow_id:`; Tauri-Kommandoargumente sind camelCase → `flowId:`. Fehler war
  „missing required key flowId“. Andere Commands geprüft, nur flow_open betroffen.
- **examples/sclm.json API-v5.0-Audit:** `won`→`result.won[].name`, Phantom-Feld
  `result.item_name` entfernt, c_results ohne Roh-`won`, Creator-Flow um
  `regenerate_codes`-Screen (on_submit→open, neuer Code-Re-Entry) erweitert.
  Manifest läuft im Test `examples_sclm_manifest_validates_with_open_flow`.
- Release-Lage: 0.5.44 live (Updater zeigt public-Repo); Hotfix zunächst nur im
  Dev-Stand, wandert mit 0.5.45 in die Releases.

## 10. Feedback-Runde Sebo Live-Test (2026-11-04, kanonisch)

11 Punkte aus dem Participant-Flow-Livetest; Karten `t_*` (Board architects-overlay,
builder), D-A=Rust/Manifest `feat/sclm-flow-behavior-v3`, D-B=Frontend `feat/sclm-ux-v3`:

1. **Frische beim Öffnen + Refresh:** Flow-Screens fetchen beim Betreten immer
   frisch (kein Poll-Cache für Flow-Treffer); Client-Poll nach `poll_seconds`
   (wishes: 60) + „Aktualisieren“-Button. (D-A serverseitig, D-B Client)
2. **Resume-Logik:** eigener Teilnehmer hat nicht-leere `wishes` → Öffnen landet
   auf `progress_screen` (wait), sonst open_screen (wishes). Manifest-Extension im
   open-Block: `progress_path` (Pfad auf matched-participant-Objekt) +
   `progress_screen`. (D-A)
3. **Styling-Pflicht:** gesamter SCLM-Bereich (Flow-Screens, Listen, Warteseite,
   Landliste) an Overlay-Look & Feel angleichen — vorhandene Muster (chip-Tabs
   `market.tab_*`, Suchergebnis-/Trade-Route-Listen, Theme-Variablen, dunkel). (D-B)
4. **Keine ID-Spalten:** Pin-/Plumbing-Mapping-Keys (loot_id, participant_id, rid,
   cid, sid) werden in Flow-Listen nicht als Spalten gerendert. (D-B)
5. **Eine Überschrift pro Ansicht:** Connector-Namen-Header („SCLM-Verlosungen“)
   und Flow-Titel („Teilnehmer“/„Ersteller“) entfallen; nur Screen-Titel. (D-B)
6. **Esc-Navigation statt Buttons:** keine „Zurück/Esc“-Buttons im SCLM-Mode.
   Esc: aus Flow → Landliste; aus Landliste → Mode verlassen. (D-B)
7. **Claim-Code-Layout:** pcode-Override-Feld als eigene Zeile unterhalb der
   Bestätigungs-Actions, großzügig platziert, nicht hervorgehoben. (D-B)
8. **Warteseite neu:** Infozeilen + alle Kopier-Buttons raus; zentrierte
   Ladeanimation + „Warten auf die Verlosung…“; darunter Wunschliste
   (state-Key `wished`, von D-A in FlowOpenOutcome.state injiziert: matched
   participant wishes → loot-Namen) + „Wünsche ändern“ (nur vor Ziehung). (D-A Daten, D-B UI)
9. **Keine Zurück-Buttons innerhalb der Verlosung** — auch nicht results→wait.
   Navigation voran nur über Actions, zurück nur Esc→Landliste. (D-B)
10. **Landliste ohne Code/ID-Zeile** unter dem Verlosungstitel. (D-B)
11. **Gewinnübersicht:** gleiche Items gruppieren (Name × Anzahl), Stil wie
    Overlay-Listen, nur Name (+Menge) als Spalten. (D-B)

Vertragsnote: D-A/D-B schneiden disjointe Dateien (D-A: crates/connector,
src-tauri/src/connector*.rs, examples/sclm.json, Tests; D-B: src/lib/*.svelte,
+page.svelte-Rendering, locales). Merge-Reihenfolge: D-A vor D-B.

## 11. Feedback-Runde 2 Sebo Live-Test (2026-11-04, kanonisch)

Karte E (Frontend, `feat/sclm-ux-v4`), Manifest unverändert. API-Fakten dafür:
`rid` = `session.results[].id` (pro Teilnehmer-Ergebnis; `result_id` ist bereits im
Keystore gepinnt), `collected`/`waived` sind ONE-WAY pro Ergebnis (Teilnehmer setzt,
nur Admin/Ersteller zurücknimmt — API.md §12/13.4); PATCH `/api/session/{sid}/raffle/{rid}`.

1. Collect/Waive-Buttons brauchen vorgelagerten Zeilen-Klick (rid-Pin leer) — rid
   kommt direkt aus Ergebnis/Keystore, Buttons laufen ohne Auswahl.
2. Ein Ergebnis = eine Entscheidung: nach collected → nur "Abgeholt ✓", nach waived →
   nur "Verzichtet"; beide Buttons durch Status ersetzt. Ersteller-Reset → frischer
   Fetch → Buttons wieder da.
3. Gewinnübersicht: nur Item-Name prominent; keine Zahlen/Striche, keine
   WAIVED/COLLECTED-Spalten. (Aktuell rendert bei Sebo ein List-Zweig statt des
   results-Zweigs — Branch-Kette/debuggen.)
4. Gruppierung gleichnamiger Gewinne (×N) greift nicht — Ursache finden (resultGroups
   hängt an mapped-Form; Array-vs-Object-Fall!) und nachweisbar fixen.
5. Wunschauswahl: Item-NAME prominent, Grade/Art/Größe dezent dahinter.
6. Nach Wunsch-Übernahme sofort Wünsche auf Warteseite (state.wished fehlt bis
   Re-Open — Action-Kette muss flow_open erneut ausführen bzw. State frisch setzen).
7. Reiter: Teilnehmer-Reiter nur bei geöffnetem Flow; Ersteller-Reiter komplett
   aus (Range stillgelegt). Landliste ohne Reiter erreichbar via Hotkey/Code.
8. "Zurück zur Liste"-Button im Reiterstil neben Teilnehmer-Reiter zurück (neben Esc).

## 12. Stand nach Korrekturrunde 2 (main bca6906, 2026-11-04)

Karte E (t_102e220d, Branch feat/sclm-ux-v4 @ 4ea9a15, Review APPROVED 18/18-Sonde)
gemerged + Reviewer-Finding 1 selbst gehärtet (Actions-Block aus bei resultsPending).
Dauerhafte Technik-Fakten:
- Rust `screen_as_view`: ScreenType::Results → ViewType::Table → `map_view` liefert
  results-Screens als ZEILENARRAY (Skalare nur in Zeile 0; {result:null} → leerer
  Array = „noch offen“, resultsPending). Frontend normalisiert via
  `connector.ts normalizeResults/groupNames` (names nie Flags; rid/waived/collected
  als scalars ins fstate → Action-rid ohne Zeilenauswahl).
- Manifest-Mapping-Keys kommen aus Rust-BTreeMap alphabetisch sortiert im DTO an —
  Hauptspaltenwahl (name-first) ist FRONTEND-Verantwortung (listColumns).
- collected/waived ist pro ERGEBNIS (rid), one-way; UI: Statuschip ersetzt beide
  Buttons; Ersteller-Reset → Frische-Fetch → Buttons zurück.
- Ersteller-Reiter per `creatorTabsDisabled=true` stillgelegt (Code erhalten).
Visuelle Abnahme Runde 2: Sebo. Release 0.5.45 weiterhin nur nach explizitem GO.

## 13. Username-Bereinigung + Klartwert-Anzeige (2026-11-04, direkt auf main)

Sebo Runde 3: (a) SCLM-Benutzername wird beim Speichern自动 leerzeichenfrei
(alle Whitespace-Charaktere), (b) der gespeicherte Name ist kein Geheimnis und
wird in den Settings angezeigt (nicht mehr nur "bereits gesetzt").
Umsetzung: connector.rs — strip_connector_ws/name_key/normalized_setting_value;
NICHT-geheime settings_fields werden bei Save UND Read (settings_field_values,
declared_setting_values, ConnectorEntry.settings_values ans Frontend) bereinigt;
secret:true-Felder bleiben byte-treu (Secret-Vertrag Spec 5.1 unverändert).
Name-Matching (scan + flow) vergleicht leerzeichenfrei+lowercase BEIDE Seiten —
Web-seitig mit Innen-Leerzeichen angelegte Teilnehmer matched weiterhin.
Frontend: Draft-Vorbesetzung aus settings_values + Server-Echo nach Speichern
(zeigt den bereinigten Wert). SCLM-Server vergleicht lower(name), Innen-Leerzeichen
bleiben dort erhalten — die Overlay-Bereinigung ist einseitig bewusst.

## 14. examples/sclm.json ist jetzt lokal (2026-11-04)

Das SCLM-Manifest ist ein internes Plugin und wurde aus allen Repo-Historien
(inkl. öffentlichem GitHub, per filter-repo) entfernt; main ist dort umgeschrieben
(50acbb2 → bde2650, Tags neu gesetzt). Die Datei liegt lokal als
`examples/sclm.json` (untracked, in .gitignore) plus Backup
`~/.config/architects-overlay/plugins-sclm-backup.json`. Probe-Skripte in
docs/scripts/ funktionieren mit der lokalen Datei unverändert weiter.
Feature-Branch-Zweige in den privaten Repos wurden nicht umgeschrieben (alle
längst in main gemerged); der öffentliche Verlauf enthält die Datei nie wieder.

## 15. Ersteller-Cockpit v1 — kanonische Spec (Sebo 2026-11-04, entschieden)

API-Fakten (API.md v5.0, verifiziert): POST /api/session/create ohne Auth, Response
{id, admin_code, join_code} — Codes nur einmal sichtbar, MUSS sofort in den
Session-Keystore. PATCH /settings teilbar (name, max_wishes 1–10 API / UI 1–6,
total_revenue, locks). Admin darf Wünsche jedes Teilnehners OHNE pcode vollständig
ersetzen (POST /wishes, loot_ids-Array; max_wishes-Grenze prüft die API im Admin-Pfad
NICHT — Overlay muss sie durchsetzen). Loot: Dedupe name|type|grade|size (qty steigt),
qty-Delta min 1, for_sale=true löscht Wünsche des Items, sale_revenue optional,
DELETE kaskadiert Wünsche. Commodities: Dedupe lower(name), scu-Float, qty-Delta.
Ziehung POST /raffle (nur Session-Admin, Content-Type json + {}; 400 ohne Teilnehmer
oder unverkauftes Loot; Neustart UEBERSCHREIBT Ergebnisse). PATCH /raffle/<rid>
collected/waived true/false — falsezuruecksetzen nur Admin. Statistik aus Session-Objekt:
Teilnehmer = participants.length, zu verlosende Einzelstuecke = sum(qty) aller
for_sale!=1, Wuensche = sum(len(participants[].wishes)). total_revenue (Waren +
Loot-Verkauf ueber commodity_summary) gibt es nur als Gesamtsumme — pro-Kopf-
Aufteilung ist reine Overlay-Anzeige (Summe / Teilnehmerzahl).

Sebo-Entscheidungen (verbindlich):
1. Erstellen: Button "Verlosung erstellen" neben ↻ in der SCLM-Hauptansicht, kleines
   Modal mit nur dem Namensfeld (+ Bestätigen/Abbrechen; max_wishes-Default 3, Feinschlag
   im Übersicht-Tab). Kein :nV-Kommando.
2. KEIN permanenter Ersteller-Reiter. Erstellte Verlosungen erscheinen in der
   Meine-Verlosungen-Liste (Scan + Keystore-Vereinigung: eigene Sessions mit
   admin_code werden immer ergänzt, auch ohne Teilnehmer). Listeneintrag mit
   gespeichertem admin_code zeigt Button "Ersteller" (öffnet Creator-View mit
   gespeichertem Code) statt/zusätzlich "Öffnen" (Öffnen bleibt, wenn pcode vorhanden).
   Admin-Code direkt in der Code-Eingabe eingegeben → Session wird per is_admin=true
   erkannt und direkt in der Creator-Ansicht geöffnet (join_code → Teilnehmer-Flow wie bisher).
3. max_wishes-Auswahl im Overlay 1–6 (Web-Limit), API-seitig 1–10 — Auswahl-Liste als
   Konstante pflegen, Erweiterung auf 10 soll möglich bleiben. Kein "Verlosung löschen"
   im Overlay (nur Web-UI).
4. Creator-Ansicht = Cockpit mit PERMANENTEN Sub-Reitern: Übersicht · Teilnehmer · Loot ·
   Waren · Verlosung. Freier Wechsel jederzeit, "Zurück zur Liste"-Chip wie Teilnehmer-Flow,
   Esc-Kaskade. Sub-Reiter-Stil = bestehende Overlay-Tab-Optik.

Manifest-Vertrag (NEU, generisches Flow-Feature — auch für künftige Connectors):
flows[].tabs = [ {"id","screen","label":{de,en}} … ] 1–6 Entries. Hat ein Flow tabs,
rendert FlowRenderer die Sub-Reiter-Leiste permanent; initiale Tab = erster Eintrag;
Tab-Wechsel = Screen frisch fetchen (Fresh-Fetch-Regel gilt pro Tab-Betreten).
Flow ohne tabs = bisheriges Wizard-Verhalten unverändert. flow.open bleibt Einstieg
(code_entry oder automatischer Code aus Keystore).

Creator-Screens (Manifest, flow "creator"):
- c_overview (detail): Teilnehmercode + Copy-Button, Erstellercode + Copy-Button
  (Ausnahme der No-Copy-Regel: nur Codes auf Creator-Screens, dokumentiert),
  max_wishes-Select 1–6 (PATCH settings, sofort), 3 Statistikkarten (Teilnehmer,
  Einzelstücke, Wünsche) im Overlay-Kartenstil; "Codes neu generieren" bleibt vorhanden
  (Confirm-Modal, Warnung: alte Codes sofort ungültig; Keystore muss neu befüllt werden).
- c_participants (list): Name prominent, Wünsche-Dezent (Anzahl + Namen), Aktionen:
  "+ Teilnehmer" (Modal), Umbenennen (Modal), Entfernen (Confirm-Modal), "Wünsche
  bearbeiten" (Modal: Loot-Mehrfachauswahl, Grenze max_wishes, Speichern = POST wishes
  full-replace; leere Liste löscht).
- loot (list): Name prominent + qty, Type/Grade/Size dezent, wish_count, sale-Status;
  Aktionen: "+ Gegenstand" (Modal: Name + optionale size/type/grade + qty), qty +/−,
  Verkauf an/aus (an-Modal fragt sale_revenue ab, optional), Entfernen (Confirm),
  "Teilnehmern zuordnen" (Modal: Teilnehmer Liste nur mit freien Wunsch-Slots
  (max_wishes − aktuelle Wünsche), Mehrfachauswahl, Zuordnung = POST wishes je
  Teilnehmer (bestehende Wünsche + neues Loot), bereits zugeordnete Teilnehmer dezent
  markiert).
- commodities (list): name + scu; "+ Ware" (Modal), scu +/−, Entfernen (Confirm);
  Kopfzeile: Gesamt-Erlös (commodity_summary.total_revenue) + "pro Kopf: X SCU/₵-Anzeige"
  (total_revenue / Teilnehmerzahl, nur Anzeige, bei 0 Teilnehmern "—").
- c_results (results): Vor der Ziehung: Button "Verlosung starten" (Confirm-Modal;
  Warnung falls raffle_done: Ergebnisse werden UEBERSCHREIBEN). Nach Ziehung: Liste
  pro Teilnehmer (Name prominent, Items gruppiert ×N), Statuschip_abgeholt/verziehen
  oder Buttons Abgeholt/Verzichtet; Admin-Extra "Zurücksetzen" (PATCH collected/waived
  false) — nach Reset erscheinen die Buttons wieder (frischer Fetch). Kein Neustart-
  Button der Ziehung im Cockpit (nur die reguläre Start-Aktion mit Warnung).

Optik/Regeln: Name immer prominent (Name-first listColumns-Regel), Plumbing-IDs nie
als Spalte, Formulare als Modal-Popup über der Liste (Liste dahinter bleibt unbewegt,
Toast-Meldungen im Vordergrund), de/en Locales vollständig.

Bauaufteilung: F = Rust+Manifest (connector.rs, examples/sclm.json, keystore, Tests);
G = Frontend (FlowRenderer tabs+Modals, ConnectorViews Ersteller-Buttons+Create-Modal,
connector.ts, locales). Merge F vor G.


## 16. Merge F + Fixture-Fallback (2026-11-05, Omar, main)

- F (t_e2f1f346) Review Runde 2 APPROVED @ d1a3343 -> main-Merge d7bd578 (--no-ff).
- Neuer Manifest-Vertrag flows[].tabs (5 Cockpit-Tabs) ist jetzt Backend-Standard;
  participant-Flow unveraendert. examples/sclm.json lokal aktualisiert (Stand R2,
  max_wishes-Select auf c_overview, 1-6), Backup nachgezogen.
- Reviewer-R1-Hinweis behoben: Manifest-Validator-Test fand gitigoriertes
  examples/sclm.json auf Frisch-Klonen/CI nicht (Panic). Fix: eingecheckter
  Snapshot src-tauri/tests/fixtures/sclm-manifest.json als Fallback im Test.
  REGEL: bei jeder Manifest-Pflege examples/sclm.json UND Fixture synchron halten.
- G (t_3f2356c7, Frontend) laeuft; Merge G -> main nach Review, dann lokaler
  Abnahme-Build. Release nur mit Sebo-Autorisierung.


## 17. Cockpit-Modal-Mausfix (2026-11-05, direkt auf main)

- Sebo-Runde 2: Cockpit-Modals schlossen bei jedem Maus-Klick (nur Tastatur
  bedienbar). Root-Cause: .fmodal-card ohne eigenes position -> der absolute
  .fmodal-backdrop malt im Paint-Order UBER die Karte (absolute Layer >
  normaler Flow) und jeder Klick traf den Backdrop-Button (closeModal).
- Fix: .fmodal-card position:relative + z-index:1 (FlowRenderer.svelte).
  .cmodal-card (ConnectorViews) hatte position:relative schon immer — daher
  war das Create-Modal nie betroffen.
- DAUERREGEL fuer jede Modal-Ebene mit Backdrop-Flaeche: die Inhaltskarte
  braucht position:relative (+ z-index), sonst frisst der Backdrop die Maus.


## 18. Cockpit-Autocomplete-Fix (2026-11-05, direkt auf main)

- Sebo-Runde 3/4: Autocomplete-Vorschlaege ungefiltert + Loot-Name ohne Echo.
- Manifest (examples + Fixture, lokal importiert): Autocomplete-Endpoints
  /api/{participants,items,commodities}/search erhielten '?q={q}'-Template +
  regel-eigene 'params'[{key:q}] — der Validator verlangt die Deklaration
  REGEL-INTERN (check_autocomplete in crates/connector/src/validate.rs),
  Screen-Level-params reichen NICHT. Fill: Loot bekam 'name':'name',
  participants/commodities 'name':'' -> 'name':'name'.
- Frontend (FlowRenderer.svelte): searchAC filtert Treffer zusaendlich
  client-seitig (Contains, case-insensitive); pickAC echoe das Treffer-Label
  immer ins Ausloeser-Feld, wenn das Fill dafuer fehlt (rohe String-Treffer
  der participants-/commodities-Suche fuellen sonst NIE ein Feld —
  applyAutofill sieht in einem String kein 'name'-Feld).
- Lehre fuer spaetere Manifest-Pflege: String-Treffer-Autocomplete ohne
  Label-Echo im Frontend ist wertlos; Fill-Mappings setzen Objekt-Treffer
  voraus.
## 19 Loot-HTTP-500: Diagnose + Qty-Klampe + 5xx-Logger (Omar, 2026-11-05)

Live-Sonden gegen sclm.site (throwaway-Sessions, geloescht/gesperrt): Der Server
panikt mit **nacktem HTTP 500 (HTML, kein JSON)** genau dann, wenn ein JSON-
Zahlenfeld (z.B. qty) **jenseits int64 range** liegt — z.B. eine 26-stellige
Menge. `i64::MAX` selbst ist noch 200, ein int64-umspannender Arbitrary-Int
bricht die DB-Schicht. Alle ueblichen Loot-Feld-Kombinationen (leer/fehlend/
Umlaute/Striche/qty=0/negativ/nach der Ziehung) sindServer-seitig sauber (200
oder klaer 400 mit Servertext) — ein 500 ohne Servertext ist damit ein
Overflow-/Panic-Verdaechtiger.

Overlay-Haertung (`src-tauri/src/connector.rs`):
- `normalize_qty_value` klemmt jede Menge auf max **999_999** (Float-Saettigung
  `1e26 -> i64::MAX` wird so entschärft); Tests decken String-/Number-/
  Saettigungsfaelle ab.
- `submit_form_request` loggt bei **Status >= 500** via `log::warn!` die Zeile
  `connector 5xx: METHOD PFAD body=... response=<16 Zeichen gekuerzt>` ins
  User-Journal (`journalctl --user -u architects-overlay`). Header-Werte
  (Session-/Admin-Codes) werden dafuer **niemals** geloggt. Damit haelt das
  Overlay beim naechsten 500 die exakte Request-Huelle fest, statt raten zu
  muessen.

## 20 'Ungültiges JSON' in der Plugin-Hauptansicht: Server-Root-Cause (Omar, 2026-11-05)

Das Overlay pullt für die Hauptansicht `GET /api/sessions`. Dieser Endpoint panikte
mit nacktem 500-HTML — Auslöser war eine meiner Diagnose-Wegwerf-Sessions mit
Übergabe-Daten (negativer qty außerhalb int64 + 1e29-scu-Ware, beide vom Server
mit 200 gespeichert). Nach `DELETE /api/session/oy7efdcs` und `.../buolv8wh` ist
`/api/sessions` wieder 200. Overlay-Seitig: fetch_request zeigt bei Nicht-JSON jetzt
'Status + Serverantwort-Ausschnitt' statt roher serde-Text und loggt 5xx GETs.
SERVER-TO-DO (SCLM-Backend, Sebo): qty/scu-Eingaben außerhalb int64/sinnvoller
Grenzen mit 400 ablehnen statt speichern; /api/sessions serialize-härten.
Restliche Wegwerf-Sessions ohne Overlay-Bezug, per Site-Admin löschbar: uxe v7o98 ('Omar-Diag2', leer),
9xmcp09y ('noop', leer), 5urkexp0 ('Omar-Diag4', 2 Loot/2 Teilnehmer, Ziehung offen).

## 21 Loot-HTTP-500: Endgueltige Root-Cause (Omar, 2026-11-05, Logger-Treffer)

Release-Logger (§ lib.rs Release-Zweig) lieferte die exakte Overlay-Request-Huelle:
`connector 5xx: Post /api/session/<sid>/loot body={"grade":"A","name":"FR-86","size":3,"type":"MIL"}`
— `size` als JSON-NUMBER. Live reproduziert: POST /loot mit size|type|grade als NUMBER ->
nackter 500 HTML; alle als STRING -> 200. Mechanik: Server baut Loot-Key
'name|type|grade|size' (und Participant-Namen) als Saiten; Zahlen-Konkatenation panikt.
Autocomplete fuellt size aus dem Katalog als Number (Katalog-'FR-86' hat size 3),
Svelte number-Bindings senden Numbers — daher der Nutzerfall.

Overlay-Seitig: harden_write_body coerciert name/type/grade/size-Zahlen zu Strings
(qty/scu/delta bleiben Zahlen; Tests decken beide Arme ab). Release-Build hat jetzt
tauri_plugin_log (Warn->stdout->journalctl --user -u architects-overlay).

SERVER-TO-DO (SCLM-Backend, exakte Repro):
POST /api/session/<sid>/loot -H 'X-SCLM-Code: <admin>' -d '{"grade":"A","name":"X1","size":3,"type":"MIL"}'
-> 500. Fix-Richtung: name/type/grade/size bei Ein- und Auslieferung auf String
normalisieren (str()) oder NUMBER mit 400 ablehnen; gleiche Felder bei /participants.
Nebenbefund Journal: 'connector keystore_fields: creator:c_overview ohne sid-Param'
(Keystore-Write uebersprungen, kosmetisch) + stale Keystore-Eintrage geloeschter
Sessions (scan-Warnung 404, Eintraege nhtgi8k3/buolv8wh aus Keystore genommen).

## 22. Sid-Fallback im Keystore-Write (Nebenbefund aus §21)

Journal showed: 'keystore_fields: creator:c_overview ohne sid-Param — Keystore-Write
uebersprungen'. All creator cockpit screens parameterize only {code}; the writer
conventionally required {sid}, so keystore_fields (e.g. name rename via settings
modal) never persisted from c_overview.

Fix (write_screen_keystore): if sid param missing, derive sid from the local
keystore entry whose join_code or admin_code matches the {code} param. Unit test
write_screen_keystore_derives_sid_from_code_when_sid_param_missing covers the
join_code match. Stale keystore entries of deleted sessions (nhtgi8k3, buolv8wh —
source of repeated scan 404 warnings) were removed from both connector-state and
connectors keystore file (backup .bak-20261105).

Deploy window for the whole round (coercion + logger + sid fallback): service stop
-> keystore cleanup -> pnpm tauri build -> install -> service start. Manifest
examples/sclm.json unchanged in this round, no re-import needed.

## 23. Cockpit-Runde 2026-11-05 (Abends): Zuordnen-Mapping-Crash + Waehler-Modal-Act

Sebo-Livetest nach dem Coercion-Deploy: Loot-500 weg. Zwei neue Befunde:

(1) 'Zuordnen' funktionierte einmal, danach 'Mapping-Fehler: expected object at id'.
Root-Cause: participants[].wishes ist serverseitig eine Liste NACKTER loot_id-Saiten
(API.md §Participant, live bestaetigt). Das Manifest-Mapping der assign_participants-
Screen deklarierte 'wish_count': 'participants[].wishes[].id' — der .id-Drall auf
Saiten lies dot_path hart fehlschlagen und die ganze Screen-Fetch-Liste krachte, sobald
ein Teilnehmer >=1 Wunsch hielt (leere wishes-Liste war vorher ungefaehrlich, darum
klappte die erste Zuweisung noch). Fix doppelt: dot_path Key-Token auf Nicht-Objekt =
leerer Treffer statt Screen-Crash (crates/connector/src/mapping.rs, tolerante DTO-Regel
verbreitert; '[]'/'[i]' auf falsche Form bleibt harte Fehlerform) + Manifest: wish_count
aus assign_participants-Mapping entfernt (examples/ Fixture/ Backup synchron —
Manifest-RE-IMPORT noetig). Serverseitig existiert kein participant wish_count-Feld;
SERVER-TODO docs/SCLM-SERVER-TODO.md Punkt 4 (additiv wish_count je Participant).

(2) 'Wuensche bearbeiten': Auswahl moeglich, Speichern ohne Wirkung, danach
'Unbekannter View.' neben '+ Teilnehmer'. Root-Cause (rekonstruiert + live eingegrenzt):
Der modalSaveMulti wishes-Zweig rief runAction NACH closeModal; runAction leitet die
Invoke-Screen-ID aus 'shown' ab = TAB-Screen c_participants. Der confirm-Act haengt im
Manifest aber an der Modal-ZIEL-Screen wishes_for -> RustAction-Lookup scheiterte ->
error.connector.view_unknown ('Unbekannter View.'), der POST lief nie raus. Live-
Gegenprobe: Admin-POST /wishes {participant_id, loot_ids:[...], mode:wish} = 200
{ok:true} (Server nimmt das Overlay-Request-Format an; loot[].wish_count existiert als
int). Fix: runAction(a, extra, screenOverride) — modalSaveMulti uebergibt die Ziel-
Screen-ID (t.id, so wie der assign-Zweig es schon immer tat); Modal schliesst erst nach
dem POST. Diagnose-Haertung: view_unknown-Faelle loggen jetzt Warn-Logs (fetch +
screen action lookup) ins User-Journal ('connector screen action: keine Action ...').

Wegwerf-Session Omar-Diag7 (qposmvsw) nach den Proben per Admin-Code geloescht.

## 24. Verkauf-Interaktion 2026-11-05b (Sebo-Livetest)

Befunde (Overlay-Frontend, manifest-gesteuert):
(1) 'Verkauf aus'-Klick stumm: runAction gate 'actionIsEnabled(a)' evaluierte
enabled_if nur gegen fstate (Flow-Zustand). enabled_if auf Zeilen-Aktionen
referenziert Screen-Mapping-Keys (for_sale), die nur in der ZEILE existieren
(rowCtx(row) wird als extra uebergeben, war aber ungelesen). sale_off
(equals true auf fehlenden fstate-Key) fiel immer durch — Button sichtbar,
nichts passierte. Server selbst in Ordnung: PATCH {for_sale:false} uebergibt
1->0 korrekt (live verified Omar-Diag8). Fix: Gate wertet
{...fstate, ...extra} (Zeile gewinnt).
(2) Verkauf <-> Wuensche (live verified Omar-Diag9): Server zieht for_sale-
Items korrekt aus der Ziehung raus; Server LOESCHT bestehende Wuensche
automatisch beim Verkauf-Markieren; NEUE Wuensche auf Verkauf-Items werden
serverseitig aber angenommen (200, nie gewinnbar) -> Overlay sperrt sie:
Manifest: participant:wishes + creator:wishes_for Mapping += for_sale;
Zuordnen-Nav am Loot-Tab enabled_if for_sale!=true. FlowRenderer:
Tab-multi-Checkboxen und Wuensche-Modal-Zeilen mit truthyFlagVal(for_sale)
is-off/deaktiviert + Hinweis 'im Verkauf — nicht verlosbar' (i18n
creator.sale_hint de/en); modalSaveMulti wishes-Zweig und confirmMulti
filtern Verkauf-Zeilen aus den POST-IDs raus (Vorauswahl alter Wuensche
bereinigt sich beim naechsten Speichern). MANIFEST-RE-IMPORT NOETIG.
