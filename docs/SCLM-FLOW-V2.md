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
