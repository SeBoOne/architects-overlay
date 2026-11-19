# Crafting Mode — Plan (M8)

> Status: Welle 1 in Vorbereitung. API: Strata Public API v1.0.0
> (Stand 2026-11-19 geprüft — UNVERÄNDERT gegen 2026-11-17: weiterhin nur
> die 6 Endpoints ores/locations/ore-locations/location-ores/items/craft-cost).
> Schwester-Plan: [MINING-MODE-PLAN.md](MINING-MODE-PLAN.md) (M7).

## API-Realität (Stand 2026-11-19)

**Vorhanden:**
- `GET /api/public/items` — 516 craftbare Items (id, name, slug, category)
- `GET /api/public/craft-cost/{item}` — craftTime + costs[]
  ({material, oreId, quantity, unit: SCU|gems|items, minQuality})
- Einheiten-Regel: Schiff-Erze = `SCU`, FPS/Ground-Vehicle-Erze = `gems`
  (1000 gems = 1 SCU), Nicht-Erze = `items` — NIE mischen/vergleichen.

**FEHLT (User-Wünsche ohne Datenquelle — harte Lücken):**
1. Blaupausen-Herkunft („durch Missionen erhältlich") — kein Endpoint/Feld.
2. Item-Details je nach Gegenstand (Größe/Munitionsart/Klasse/Grade/etc.) —
   API kennt nur name/category/slug, KEINE Statistiken.
3. Qualitätsregler mit Stat-Änderungs-Berechnung — Item-Stats UND
   Qualitäts-Kurven fehlen komplett.

→ Entscheidung (User-Antworten blieben aus, Defaults dokumentiert):
Welle 1 mit den API-Daten bauen; die Lücken warten auf Strata-API-Erweiterung
(„früher oder später" — Spec im Auge behalten, falsche Erwartungshaltung
gegenüber Testern vermeiden: Changenotes sagen klar, was drin ist).

## Entschiedene Defaults (jederzeit änderbar)

1. **Modus-Taste F6** (F5=Mining, F1–F4/F11/F12 belegt).
2. **Qualitätsregler als PLANUNGSRECHNER** (bis Stat-Daten kommen):
   Ziel-Qualität wählen (Slider 0–1000) → Anzeige der geforderten
   MINDEST-QUALITÄT der Inputs = max(minQuality) über alle Kosten; je
   Material grün/rot markiert ob die geforderte Qualität ≥ dessen
   minQuality erreicht wird. KEINE Stat-Versprechen — es ist ein
   Machbarkeits-Check, kein Stat-Prediction.
3. **Welle 1:** Suche + Kategorie-Filter + Material-Detail + Planungsrechner
   + Mining-Verknüpfung. Welle 2 (wenn Datenquelle vorhanden): Blaupausen-
   Missionen, Item-Stats, echter Stat-Rechner.

## Architektur

- `crates/strata-client` ERWEITERN (kein neuer Client): DTOs
  Item/ItemsResponse/CraftCostResponse nach Spec, Cache-TTL: items 7d
  (Patch-zyklisch), craft-cost 7d.
- **ToS wie Mining:** eigener `celd_`-Key (Settings-StrataSection geteilt),
  Attribution-Footer „Data from celd.space" überall, Cache-first.
- F6-Modus im Rail/F-Keys-System (In-App-Keyhandler, KEINE Hyprland-Binds).
- **Modi-Verbindung (Kern der User-Vision):** im Craft-Detail jedes
  Bedarfserz anklickbar → springt in den Mining-Modus (F5) mit dem Erz
  vorgewählt (Orte-pro-Erz-Detail direkt offen). Umgekehrt später:
  Mining-Liste aus Craft-Kosten befüllen (Welle 2+).

## Welle 1 Scope (Karten)

- **K1 strata-client erweitern:** Items-/CraftCost-DTOs + Endpoints +
  Cache-TTLs, Unit-Tests gegen Spec-Examples + ggf. Live-Fixtures.
- **K2 Crafting-UI (F6):** Suche (Typeahead Name/Slug, leeres Feld =
  Gesamtliste), Kategorie-Filter (dynamisch aus Daten), Item-Liste als
  kompakte Zeilen; Detail: Materialliste (Menge + Einheit farblich
  getrennt nach SCU/gems/items, minQuality-Badge), Craft-Zeit,
  Qualitäts-PLANUNGSRECHNER (Ziel-Slider 0–1000 → geforderte Input-Qualität
  pro Material grün/rot), Attribution-Footer, Key-Missing-State wie Mining.
- **K3 Modi-Verbindung:** Bedarfserz → Mining-Modus-Sprung (Erz vorgewählt,
  Orte-Detail offen); Rückweg per ESC/Back.

## Welle 2 (2026-11-19, durch SC-Wiki-Live-Beleg möglich)

SC-Wiki-API (api.star-citizen.wiki, live geprüft am Greatsword-Blueprint)
liefert ALLES was Strata fehlte:
- `items?filter[is_craftable]=1`: size, grade, class, mass, ammunition,
  vehicle_weapon (vollständige Schadens-Stats), description (de_DE!)
- `blueprints/{uuid}`: **unlocking_missions_grouped** (Guaranteed/Chance +
  reward_scope), craft_time_seconds (= Strata), ingredients (SCU),
  dismantle_returns, game_version
- **Qualitäts-System:** tiers[].requirements → groups (FRAME/CYCLER) →
  resources mit min_quality + **modifiers[]** (property_key, quality_range
  0–1000, modifier_range at_min/at_max z.B. 0.9→1.1, value_range_type
  „linear") + summary_properties (better_when)

→ Karten: M8-K4 scwiki-client erweitern · M8-K5 Item-Details + Missionen ·
M8-K6 echter Stat-Rechner (Modifier-Interpolation). Kreuzreferenz:
SC-Wiki-Item-Name ↔ Strata-Item-Name; Blueprint via item.blueprint[0].uuid.
Fehlender Match degradiert (kein Hard-Fail). Attribution zusätzlich
„Data from star-citizen.wiki".

## Offen / später

- Mining-Listen (Welle 2 M7) + Craft-Bedarf-Import zusammenführen.
