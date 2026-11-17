# Mining Mode — Plan (M7)

> Status: Welle 1 in Vorbereitung. API: Strata Public API
> (https://strata.celd.space, OpenAPI v1.0.0, Stand 2026-11-17).
> Crafting Mode folgt in einer eigenen Runde — beide Modi sind von Anfang an
> auf Zusammenwirken geplant (Craft-Bedarf → Mining-Liste).

## Datenquelle: Strata Public API

| Endpoint | Inhalt | Mining-Nutzung |
|---|---|---|
| `GET /api/public/ores` | Alle Erze (id, name, tier, category, scanSignature, instability, resistance) | Erz-Suche, Signatur-Chart |
| `GET /api/public/locations` | Alle abbaubaren Orte (id, name, system, type) | Ort-Suche, Resolver für Listen-Optimierung |
| `GET /api/public/ore-locations/{ore}?sort=rank|concentration` | Orte pro Erz, gruppiert nach `space/surface/cave/exploration`, je mit rankPct, rank, n, concentration, tier (1–6: Negligible→Pure) | „Beste Orte pro Erz" |
| `GET /api/public/location-ores/{location}` | Erze pro Ort (concentration abs., tier, scanSignature) | Listen-Optimierung (Set-Cover) |
| `GET /api/public/items` | 516 craftbare Items | Crafting Mode (später) |
| `GET /api/public/craft-cost/{item}` | Materialkosten inkl. `minQuality` (Einheit: SCU vs. `gems`/`items` — nie mischen!) | Crafting Mode (später), Projekt-Import |

### Verbindliche Rahmenbedingungen (ToS)

- **Eigener `celd_`-Key pro Nutzer** (self-service via
  https://strata.celd.space/api-keys, Discord + verifizierte Mail + Name).
  Die App darf KEINEN geteilten Key ausliefern (Revokation folgt sonst).
  Key-Handling wie UEX-Key: Keystore (`connector_keystore`), Eingabe in
  Settings.
- **Pflicht-Attribution** „Data from celd.space" bei JEGER öffentlichen
  Anzeige der Daten (Modus-UI unten attribution-footer).
- **Nicht-kommerziell** (CIG-Fan-Regeln) — AO erfüllt das.
- **Aggressiv cachen:** Daten ändern sich nur pro Game-Patch. TTL-Cache im
  `cache`-Crate (ORES/LOCATIONS lang, ore-locations/location-ores mittel).
  120 req/60s sind mit Cache nie ein Thema.

## Entschiedene Defaults (User-Antworten blieben aus — jederzeit änderbar)

1. **Modus-Taste F5** (Lücke im Raster: F1–F4, F11, F12).
2. **Quality-800-Falle:** Die API hat KEINE Spawn-Qualität. Statt
   Qualitätsfilter eine **einstellbare Concentration-Tier-Schwelle pro Liste**
   (Default: Tier 4 „Notable"+). „mindestens Qualität 800" ist mit dieser
   Datenquelle nicht abbildbar — falls der User eine Qualitätsquelle kennt,
   kommt sie als Erweiterung dazu.
3. **Wellen-Split:** Welle 1 = Erz-Suche + Scan-Signaturen-Pins → testen.
   Welle 2 = Mining-Listen (mehrere benannte Listen, abhakbar) +
   Orts-Optimierung (pro-Erz-Bester-Ort ODER „möglichst alles an einem Ort",
   Set-Cover-Heuristik über location-ores mit Tier-Schwelle).
4. **Mehrere benannte Mining-Listen** (projektfähig; später vom Crafting Mode
   füllbar).

## Architektur

- Neuer Crate `crates/strata-client` (analog `uex-client`): Bearer-Key-Handling,
  DTOs, Endpoints. Kein Overlay-Code SCLM-/Strata-gebunden — generisch.
- Settings: neue `StrataSection` (enabled, key-ref → Keystore, tier-default).
- F5-Modus im bestehenden Rail/F-Keys-System (In-App-Keyhandler, KEINE
  Hyprland-Binds — Konvention wie F1–F4).
- Signaturen-Pins über das Pin-Content-System (M6-F1): Pin-Inhalt-Typ
  „scan-signature" — Chart-Wert + Ore-Name als Pin-Card, Pin-Verhalten
  unverändert (M4/M6-Verträge: reine Anzeige, Interaktion nur via Patch).
- Mining-Listen persistiert in `settings` (Listen mit Name + Einträgen
  {oreId, menge, abgehakt}).

## Welle 1 Scope (Karten)

- **K1 `strata-client`:** Crate, DTOs (Ore, Location, OreLocations, LocationOres),
  Bearer via Keystore, Cache-TTLs, Attribution-Feld durchreichen, Unit-Tests
  gegen DTO-Contract (Spec-Examples als Fixtures).
- **K2 Mining-Mode-UI (F5):** Erz-Suche (Typeahead Name/Slug, Filter
  category/tier), Ort-Suche (Typeahead, Filter system/type), Detail-Ansicht:
  Orte pro Erz gruppiert nach Umgebung (space/surface/cave/exploration) mit
  rankPct + tierBadge (Farbskala None→Pure), Pin-Button „Signatur pinnen" →
  Pin-Content „scan-signature". Attribution-Footer. Leere-Key-Erklärung mit
  Link auf api-keys-Seite.
- **K3 Signaturen-Pin-Rendering:** Pin-Content-Typ im Pins-System, Sichtbar
  im Pins-only-Modus wie andere Pins.

## Welle 2 Scope (nach Welle-1-Feedback)

- Mining-Listen-UI (mehrere Listen, Einträge {oreId, menge}, Checkbox),
  Bestort-Panel pro Erz (rankPct-Tabelle), „Alles-an-einem-Ort"-Vorschlag
  (Greedy-Set-Cover, Tier-Schwelle respektiert).
- Craft-Bedarf-Import (Crafting Mode follow-up).

## Mockup-Referenz (User 2026-11-17 nachgeliefert)

`docs/references/strata-abundance-dials.png` — Strata „Ore Finder"-Doku:
**Two-Ring-Dial** pro Erz-Ort:

- **Außenring (Mint) = RANK:** Füllgrad des Bogens = Rang unter den Orten
  dieses Erzes (fast voll = Top-Rank).
- **Innenring (Amber→Mint, segmentiert) = CONCENTRATION:** Anteil der Erze am
  Ort, 6 Stufen mit exakten Prozentbanden: Barely Any <1 % / A Trace 1–4 % /
  Some 4–8 % / A Decent Amount 8–15 % / Lots 15–25 % / Almost Pure ≥25 %
  (= Tier-Skala 1–6 der API, absolute Bands — überall vergleichbar).
- Zentrum: Badge mit Erz-Kürzel + Scan-Signatur-Wert.
- Ergebnisse in Sektionen (Haupt-Ziele → explorable Regions → FPS-Caves),
  jede Sektion eigene Skala — entspricht den API-Gruppen
  space/surface/cave/exploration.
- Farblage Strata (dunkelgrün-schwarz, Mint #4EE6A4 = Rank, Amber #F59E0B =
  Konzentration): **Semantik übernehmen, Farben an unser Overlay-Theme
  angleichen** (CSS-Variablen, kein Strata-Grün als Primärfarbe).
- Statische Dial-Labels entfallen (Hover-Tooltips mit exaktem rank/
  concentration %), Legendenpanel bleibt sichtbar.

## Offen / später
- Qualitäts-Quelle klären (s. Defaults #2).
- Strata verspricht evtl. mehr Endpoints „früher oder später" — Spec im Auge
  behalten (Scan-Chart-Rohdaten, Spawn-Qualität).
