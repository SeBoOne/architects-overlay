# UX-Feedback-Fixes (Live-Feedback v0.1.3, Punkte 1/6/7/8) — Bericht

> **Historisch, Stand 2026-10 (Momentaufnahme zum Zeitpunkt des Berichts).**
> Repo-/CI-Angaben unten beschreiben den damaligen Zustand — heute läuft
> Build/Release über GitHub Actions (siehe docs/RELEASE.md).

Branch: `feature/ux-feedback` (Basis `main` @ `596ad6b`, v0.1.3)
Repo: sebo/architects-overlay (Gitea, Historie) · Task: t_594c213c

Ergebnisse der parallel laufenden Backend-Karte (Punkte 2/3/4/5, Terminal-Namen,
Location-Preis-Aggregat, Dedupe, Non-Purchasable) sind hier NICHT enthalten — die
macht `feature/data-fixes` (t_7c9905f9).

## Umfang & Umsetzung

### 1. Präfix-Case (Punkt 1)
- `parseQuery()` vergleicht Präfixe case-insensitive: `:c`, `:C`, `:C `, `' ':i`
  parsen identisch wie `:c/:i/:l` (Token wird vor dem Vergleich lowercased).
- UI zeigt Kategorien lowercase: Zeilen-Icon `ICONS = {commodity:'c', item:'i',
  location:'l'}` statt vorher `C/I/L` — Eingabe und Anzeige sind jetzt konsistent.
- Unbekannte `:`-Präfixe bleiben Fuzzy-Terme (Review-Auflage A1, unverändert).

### 2. Auto-Scroll der Ergebnisliste (Punkt 6)
- `move()` scrollt nach jeder Pfeil-Navigation die fokussierte Zeile via
  `scrollIntoView({block:'nearest'})` in den sichtbaren Bereich.
- Scroll-Ziel ist `.rwrap` = Zeile **inklusive Inline-Expansion** → auch
  aufgeklappte Zeilen werden mit voller Höhe berücksichtigt.
- Gilt für Such- und Market-Liste (`scrollOfferIntoView`).

### 3. Market-Modus: HAUPT-Suchfeld statt eigenem Feld (Punkt 7)
- Eigenes `.mq`-Suchfeld im Market-Panel **entfernt**.
- `input.q` ist die einzige Suchzeile: im Market-Modus durchsucht sie die Offers
  (Platzhalter wechselt: „Marktplatz durchsuchen — Name / Typ / Terminal /
  Verkäufer …"), im Such-Modus wie gehabt die Kommodities/Items/Terminals.
- Provider (`crates/uex-client`, `apply_local`): `q` matcht jetzt Name (title),
  Typ (listing_type + operation), Terminal (location) und Verkäufer (seller)
  zusätzlich zu description — **Multi-Term-AND** (jeder Term muss in mindestens
  einem Feld treffen), case-insensitive. Läuft gegen den bestehenden Cache-Index
  bzw. die Live-Antwort — keine neuen Endpunkte, kein Schema-Change. (Das ist
  die im Task erlaubte Ausnahme „Market-Suche gegen den bestehenden Cache-
  Index"; die API selbst hat laut uex-api-reference.md §4b keinen Text-/Qualitäts-
  Filterparameter.)
- Suchfeld-Moduswechsel über Tab/M wie bisher; beim Verlassen des Market-Modus
  (Esc) sucht `runSearch()` sofort mit dem aktuellen Text → keine veralteten
  Treffer bleiben stehen.

### 4. Qualitätsfilter Min/Max (Punkt 7)
- Zwei Zahl-Eingaben (Stepper, min 0 / max 1000 / step 10, UEX-Skala) + Reset-
  Chip „Q ×" in der Market-Filterleiste.
- **Clientseitig** gefiltert (`visOffers`-Derived + `$effect`-Klemme für mSel),
  weil die UEX-API keinen Qualitäts-Parameter bietet (Referenz aus dem Research-
  Attachment, uex-api-reference.md §4b: Filter über Query-Parameter ohne
  Qualitätsangabe) — die im Task vorgesehene Fallback-Option.
- Kombinierbar mit Sortierung (Provider sortiert, der Filter ändert die
  Reihenfolge nicht) und mit Operation/Region-Kategorien.

### 5. Market-Tastatur-Navigation (Punkt 8)
- ↑/↓ navigieren die Offers (mit Auto-Scroll), Enter öffnet die Detail/Aktion
  (`ArrowRight` ebenfalls), Alt+1..0-Badges auf den ersten 10 Offers (erneut →
  zurück ins Suchfeld), Esc schließt zuerst das Detail, das nächste Esc verlässt
  den Market-Modus **zurück zur Suche** — das Overlay wird NICHT ausgeblendet.
- Sortierung/Filter per Tastatur erreichbar: `Tab` wandert nativ durch die
  Controls (alle Filter-/Sort-Chips sind jetzt `<button>` mit Enter/Space-
  Aktivierung, Qualitäts-Stepper behalten ihre nativen ↑↓; der alte Tab→Detail
  Haken ist entfernt). Shortcuts in Helper-Zeile + Fußzeile dokumentiert.
- Helper-Zeile im Market-Modus zeigt die Market-Shortcuts statt der Präfixe.

### NB-1/NB-2-Fixups (QA-Anforderung, aus Arkanis-Review Runde 2)
- Cherry-picked aus dem lokalen Fixup-Commit `7781313` (feature/arkanis-ux-
  Fixups, im Worktree des Haupt-Repos liegend): `focusOnly()` klappt NICHT mehr
  auf (Kommentar/Spec §4 stimmen jetzt), Enter öffnet `focusIdx ?? sel` (Enter
  nach Alt+Zahl trifft die fokussierte Zeile), `move()` übernimmt die Fokuszeile.

## Gate

- `cargo fmt --all` — ok
- `cargo check --all-targets` — ok
- `cargo test --workspace` — **67 passed / 0 failed** (vorher 63; +4 neue
  uex-client-Tests: q matcht Name/Typ/Terminal/Verkäufer, Multi-Term/case,
  listing_type+operation, q-leer + Sort-Kombination)
- `pnpm check` — 0 errors / 12 warnings (nur vorbestehende a11y; vorher 28 —
  Chip-span→button hat mehrere bereinigt)
- `pnpm build` — ok (adapter-static, 130 ms)
- Cargo.lock: Version 0.1.2→0.1.3 nachgezogen (main hatte Cargo.toml ohne Lock
  gebumpt) — keine Dependency-Änderung.

## Smoke (CDP, headless Chromium, `docs/scripts/ux-feedback-probe.py`)

**27/27 Checks PASS**, Screenshots in `docs/screens/07..10`:

| Check-Gruppe | Ergebnis |
|---|---|
| `:C gold` → filters=['commodity'], nur Commodities-Gruppe, Icons lowercase | PASS |
| Auto-Scroll: 25 Treffer, 15× ArrowDown → scrollTop=475, Selektion sichtbar | PASS |
| Market: kein `.mq`, Platzhalter gewechselt, Badges `1,2,…,9,0`, 14 Offers, Helper-Texte, 10 Filter-Chips als Buttons | PASS |
| Haupt-Suchfeld „tressler" → 5 Offers (Terminal-Match über Provider-q) | PASS |
| Qualität min 900 → 3 Offers; Sort „Preis ↑" per Tab+Enter → is-on + price_asc; erste Zeile 4.812 (Sort+Qualität kombiniert) | PASS |
| ↑↓-Nav (sel=2), Alt+2 → Offer 1, Alt+2 erneut → Suchfeld, Enter → Detail, Esc → Detail zu (bleibt Market), Esc → SUCHE, 0 Overlay-Hide-Calls | PASS |
| Keine Console-Exceptions | PASS |

Einschränkung (wie Arkanis-Runde, Review N2): Der Browser-Smoke speist
`smoke-stub.js` — der JS-Mirror bildet den Provider-Kontrakt (q/Sort/operation)
1:1 ab; der **echte Rust-Pfad ist durch die cargo-Tests** (uex-client
`apply_local`-Suite) belegt. Screenshots belegen damit die Svelte-Logik; ein
Live-Overlay-Lauf mit echter UEX-Antwort war wie in R1 bewusst nicht möglich
(aktive SC-Session, Overlay wäre intrusiv). Der Live-Cache wurde nicht angefasst.

## Offene Punkte

- Keine auf dieser Karte. Hinweis zur Orchestrierung: `src/routes/+page.svelte`
  ist ein Hotspot (parallele Karten editieren dieselbe Datei — 7781313 kam
  zeitgleich aus einem anderen Worktree; wurde per Cherry-pick übernommen statt
  parallel redigiert).
- Backend-Punkte 2-5 (Checkmate-Terminalnamen, Location-Preis-Aggregat, Dedupe,
  Non-Purchasable) liegen auf t_7c9905f9 (feature/data-fixes); erst nach deren
  Merge in main sollten die Branches zusammengeführt werden.
