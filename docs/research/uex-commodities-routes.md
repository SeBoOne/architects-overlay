# UEX Commodities Routes — Verifikation + Belege (Task t_851c9b2d)

> Erstellt: 2026-10-27 | Status: **verifiziert** | Quelle: Live-API-Tests + Doku

## 1. Endpoint-Verifikation

### 1.1 Grunddaten

| Feld | Wert |
|---|---|
| **URL** | `GET https://api.uexcorp.space/2.0/commodities_routes` |
| **Auth** | KEIN API-Key erforderlich |
| **Content-Type** | `application/json` |
| **Cache-Control** | `public, max-age=1800, must-revalidate` (30 Min.) |
| **Max Rows** | 500 pro Request |
| **Update Frequency** | Stündlich laut Doku |
| **UET-Datum live Probe** | 1793101614 → 2026-10-27 11:46:54 UTC |

### 1.2 Parameter

**Mindestens EINES erforderlich:**

| Parameter | Typ | Beschreibung |
|---|---|---|
| `id_terminal_origin` | int | Start-Terminal-ID |
| `id_planet_origin` | int | Start-Planet-ID |
| `id_orbit_origin` | int | Start-Orbit-ID |
| `id_commodity` | int | Ware ID |

**Optional:**

| Parameter | Typ | Beschreibung |
|---|---|---|
| `id_terminal_destination` | int | Ziel-Terminal-ID |
| `id_planet_destination` | int | Ziel-Planet-ID |
| `id_orbit_destination` | int | Ziel-Orbit-ID |
| `id_faction_origin` | int | Ursprungs-Fraktion |
| `id_faction_destination` | int | Ziel-Fraktion |
| `investment` | int | Max. Investment (Filter) |

**Test-Ergebnisse:**

```
id_terminal_origin=1              → 200 OK, 34 Records
id_terminal_origin=1, limit=3     → 200 OK, 3 Records (truncated response)
id_commodity=79                   → 200 OK, 500 Records (API-Maximum)
id_commodity=47                   → 200 OK, 120 Records
id_commodity=47 AND id_terminal_origin=1  → 200 OK, 20 Records (kombiniert!)
id_planet_origin=4                → 200 OK (works as required input)
id_terminal_origin=1, dest=45     → 200 OK, 1 Record (präziser Filter)
```

**Wichtig für Integration:** Die Feldumhüllung variiert je nach Query-Tiefe. Bei nur `id_commodity` ohne Terminal-Filter werden nur Kernfelder zurückgegeben (id, commodity_name, id_commodity, id_terminal_origin, id_terminal_destination, price_origin, price_destination, profit, score, distance — ca. 10 Felder). Für das volle Schema (76 Felder) muss mindestens ein Terminal-Filter mitgegeben werden (z.B. `id_terminal_origin=N`).

### 1.3 User-Agent-Richtlinie
UEX verlangt einen benutzerdefinierten User-Agent. Alle Tests verwendet:
```
User-Agent: Hermes-Researcher/1.0 (automation@nousresearch.com)
```

---

## 2. Vollständiges Feldschema (76 Felder)

### 2.1 Antwort-Enveloppe
```json
{
  "status": "ok",           // string
  "http_code": 200,         // integer
  "data": [...],            // array of route objects (max 500)
  "message": ""             // string (leer bei success)
}
```

### 2.2 Route-Objekt — alle 76 Felder

#### Identifikation (3)
| Feld | Typ | Beispiel | Beschreibung |
|---|---|---|---|
| `id` | `int` | `474`–`8430` | Route-UUID/Internal-ID |
| `code` | `string` | `"c4482277"` | Hash-code der Route |
| `id_commodity` | `int` | `47` (Laranite), `79` (Waste) | Commodity-ID |

#### Commodity-Details (2)
| Feld | Typ | Beispiel | Beschreibung |
|---|---|---|---|
| `commodity_name` | `string` | `"Laranite"`, `"Waste"` | Name der Ware |
| `commodity_slug` | `string` | `"laranite"` | URL-Slug der Ware |

#### Origin-Terminal (9)
| Feld | Typ | Bereich | Beschreibung |
|---|---|---|---|
| `id_terminal_origin` | `int` | filterwert | Terminal-ID (immer gesetzt) |
| `origin_terminal_name` | `string` | `"Admin - ARC-L1"` | Vollständiger Name |
| `origin_terminal_code` | `string` | `"ARCL1"` | Abkürzung |
| `origin_terminal_slug` | `string` | `"admin-arc-l1"` | URL-Slug |
| `origin_terminal_is_player_owned` | `int` | 0/1 | Spieler-owned? |
| `origin_star_system_name` | `string` | `"Stanton"` | Sternsystem |
| `origin_planet_name` | `string` | `"ArcCorp"` | Planet/Mond |
| `origin_orbit_name` | `string` | `"ArcCorp Lagrange Point 1"` | Orbit/Spezifikator |
| `origin_faction_name` | `string` | nie null | Fraktion |

#### Origin-Terminal-Flags (9)
| Feld | Typ | Bereich | Beschreibung |
|---|---|---|---|
| `has_docking_port_origin` | `int` | 0/1 | Andockstelle |
| `has_refuel_origin` | `int` | 0/1 | Treibstoffstation |
| `has_cargo_center_origin` | `int` | 0/1 | Cargodeck |
| `has_loading_dock_origin` | `int` | 0/1 | Ladedock/Frachtaufzug |
| `has_freight_elevator_origin` | `int` | 0/1 | Frachtaufzug |
| `has_quantum_marker_origin` | `int` | 0/1 | Quantenmarker |
| `is_monitored_origin` | `int` | 0/1 | Überwacht (UEE Commlink) |
| `is_space_station_origin` | `int` | 0/1 | Raumstation |
| `is_on_ground_origin` | `int` | 0/1 | Bodenstation |

#### Destination-Terminal (9)
| Feld | Typ | Bereich | Beschreibung |
|---|---|---|---|
| `id_terminal_destination` | `int` | 12–802 | Ziel-Terminal-ID |
| `destination_terminal_name` | `string` | `"Scrap - Rappel"` | Vollständiger Name |
| `destination_terminal_code` | `string` | `"RAPPE"` | Abkürzung |
| `destination_terminal_slug` | `string` | `"scrap-commodities-rappel"` | URL-Slug |
| `destination_terminal_is_player_owned` | `int` | 0/1 | Spieler-owned? |
| `destination_star_system_name` | `string` | `"Stanton"` | Sternsystem |
| `destination_planet_name` | `string\|null` | null bei 6×34 | Planet (bei Inter-System = null) |
| `destination_orbit_name` | `string` | z.B. `"Hurston"` | Orbit |
| `destination_faction_name` | `string\|null` | null bei 6×34 | Fraktion (bei Inter-System = null) |

#### Destination-Terminal-Flags (9)
Analog Origin (9 Flag-Felder): alle 0/1-int, selbe Semantik mit `_destination`-Suffix. Null-Werte nur bei `destination_planet_name` und `destination_faction_name` (je 6× null von 34 Records).

#### Standorte-/Factions-IDs (8)
| Feld | Typ | Bereich | Beschreibung |
|---|---|---|---|
| `id_star_system_origin` | `int` | 68 | Ursprung-Sternsystem |
| `id_star_system_destination` | `int` | 55–68 | Ziel-Sternsystem |
| `id_planet_origin` | `int` | 4 | Ursprungsplanet |
| `id_planet_destination` | `int` | 0–245 | Zielplanet (0 = unbekannt) |
| `id_orbit_origin` | `int` | 326 | Ursprungsorbit |
| `id_orbit_destination` | `int` | 4–397 | Zielorbit |
| `id_faction_origin` | `int` | 23 | Ursprungsfraktion (immer gesetzt) |
| `id_faction_destination` | `int` | 0–35 | Ziel-Fraktion (0 = neutral/franchise) |

#### Preissystem (11)
| Feld | Typ | Bereich | Beschreibung |
|---|---|---|---|
| `price_origin` | `int` | 229–7400 | Einkaufspreis am Ursprung (scu) |
| `price_destination` | `int` | 200–9300 | Verkaufspreis am Ziel (scu) |
| `price_margin` | `float` | −14.50–49.11 | Marge % (Gross-Margin auf Verkauf) |
| `price_roi` | `float` | −12.66–96.51 | Rendite % (auf Kaufpreis) |
| `profit` | `int` | −1800–3800 | Absolutgewinn (credits, Blackbox) |
| `investment` | `int` | 1603–14800 | Max-Investment in SCU |
| `price_origin_users` | `int` | 0–7106 | Community-Preis Origin |
| `price_origin_users_rows` | `int` | 0–5 | Anzahl User-Preise Origin |
| `price_destination_users` | `int` | 0–8505 | Community-Preis Destination |
| `price_destination_users_rows` | `int` | 0–3 | Anzahl User-Preise Dest |
| `distance` | `int` | 3–136 | Entfernung in GM (Gigameter) |

#### SCU-Bestand (8)
| Feld | Typ | Bereich | Beschreibung |
|---|---|---|---|
| `scu_origin` | `int` | 2–7 | Verfügbare SCU Origin |
| `scu_destination` | `int` | 2–2038 | Kaufbare SCU Destination |
| `scu_reachable` | `int` | 2–7 | Erreichbare SCU |
| `scu_margin` | `int` | 0–101800 | SCU-basierte Marge |
| `scu_origin_users` | `int` | 0–290 | Community SCU Origin |
| `scu_origin_users_rows` | `int` | 0–5 | User SCU Einträge |
| `scu_destination_users` | `int` | 0–203 | Community SCU Dest |
| `scu_destination_users_rows` | `int` | 0–3 | User SCU Einträge Dest |

#### Lagerbestand (2)
| Feld | Typ | Bereich | Beschreibung |
|---|---|---|---|
| `status_origin` | `int` | 2 (konstant in Samples) | Inventar-Level Origin |
| `status_destination` | `int` | 1–5 | Inventar-Level Destination |

#### Container / Meta (6)
| Feld | Typ | Beispiel | Beschreibung |
|---|---|---|---|
| `container_sizes_origin` | `string/csv` | `"1,2,4,8,16,24,32"` | Behälter Origin |
| `container_sizes_destination` | `string/csv` | `"1,2,4,8,16,24"` | Behälter Destination |
| `game_version_origin` | `string` | `"4.10.2"` | Spielversion Origin |
| `game_version_destination` | `string` | `"4.10.2"` | Spielversion Destination |
| `score` | `int` | −465–953 | UEX-Bewertung |
| `date_added` | `int` (Unix) | 1793101614 | Datum der Erfassung |

**Summe: 76 Felder.** Null-Werte nur bei `destination_planet_name` und `destination_faction_name` (Cross-System-Routes). `origin_faction_name` ist immer gesetzt.

---

## 3. Vergleich: UEX vs. K1 lokale Matrix

### 3.1 Architekturunterschied

**K1 (`compute_routes_full`):** Sucht pro Commodity den GÜNSTIGSTEN Einkauf (niedrigster buy_price) und den BESTEN Verkauf (höchster sell_price) über ALLE Terminals — also die optimale Kombination aller möglichen Paare. Ergebnis: eine Route pro Commodity mit dem maximalen Gewinn.

**UEX commodities_routes:** Zeigt vorkalkulierte Routen für spezifische Origin→Destination-Kombinationen. Kein All-Terminal-Optimierungsalgorithmus — nur die abgefragten Pärchen.

### 3.2 Feldeckerstand

| Kategorie | UEX Felder | K1 Äquivalent | Neu bei UEX |
|---|---|---|---|
| Identifikation | 3 | 3 | 0 |
| Commodity | 2 | 2 | 0 |
| Origin-Terminal | 9 | ~3 | 6 (Code, Slug, Owner, System, Planet, Faction) |
| Origin-Flags | 9 | 3 (autoload, cargo, ground) | 6 weitere Flags |
| Destination-Terminal | 9 | ~3 | 6 |
| Destination-Flags | 9 | 3 | 6 |
| Standort-IDs | 8 | 0 | 8 (vollständige Hierarchie-IDs) |
| Pricing | 11 | ~4 (buy/sell/margin/profit) | 7 (margin/roi/investment/user_prices/distance/GM) |
| SCU | 8 | 1 (sell_stock) | 7 |
| Inventory | 2 | 2 (buy_status/sell_status) | 0 |
| Container/Meta | 6 | 0 | 6 |
| **Summe** | **76** | **~15** | **~50 neue Felder** |

### 3.3 Wo K1 überlegen ist

| Aspekt | UEX | K1 | Beurteilung |
|---|---|---|---|
| Optimierung | Nur Abfrage-Paar | Beste Buy→Sell-Paar über ALLE Terminals | ✅ K1 |
| Profit-Transparenz | Blackbox-Metrik | Transparent: `sell−buy` | ✅ K1 |
| Abdeckung | Pro-Abfrage | Sofort komplett (Matrix) | ✅ K1 |
| Distanzmetrik | GM (3–136) | Proxy 0–4 | 🔄 unterschiedlich |
| Keine API-Dep | Nein (extern) | Ja (lokal) | ✅ K1 |

### 3.4 Konkreter 3-Beispiele-Vergleich (live verifiziert)

#### Beispiel 1: Laranite — ARC-L1 → HUR-L2 (Top-Route)

| Feld | UEX-Wert | K1-(Rohdaten) | Differenz |
|---|---|---|---|
| Commodity | Laranite (ID 47) | Laranite (ID 47) | ✓ identisch |
| Origin/Dest | ARC-L1 → HUR-L2 | ARC-L1 → HUR-L2 | ✓ identisch |
| price_origin | **7400** | ~7400 (PriceRow.buy_price) | ✅ kongruent |
| price_destination | **9300** | ~9300 (bestes Sell-Preis an HUR-L2) | ✅ kongruent |
| profit | **3800** | `9300 − 7400 = 1900` | ❌ UEX ≈ 2× K1-Gewinn |
| margin (%) | **20.43%** | `(9300−7400)/9300 × 100 = 20.43%` | ✅ gleiche Formel (Gross-Margin) |
| roi (%) | **25.68%** | `(9300−7400)/7400 × 100 = 25.68%` | ✅ Standard-Profit-Marge |
| status_origin | 2 | gleiches Konzept | ✓ |
| status_destination | 1 | gleiches Konzept | ✓ |
| scu_origin | 2 | PriceRow.scu_buy_avg | ✓ |
| score | **953** | — | neu |
| distance | 20 (GM) | 2 (Proxy-Hierarchie) | ❌ andere Skala |

#### Beispiel 2: Laranite — ARC-L1 → CRU-L1 (Crusader)

| Feld | UEX-Wert | K1-(Rohdaten) | Differenz |
|---|---|---|---|
| Origin/Dest | ARC-L1 → Crusader L1 | ARC-L1 → Crusader L1 | ✓ |
| price_origin | **7400** | ~7400 | ✅ kongruent |
| price_destination | **9000** | ~9000 | ✅ kongruent |
| profit | **3200** | `9000−7400 = 1600` | ❌ UEX ≈ 2× K1 |
| margin (%) | **17.78%** | Gross-Margin ✓ | ✅ |
| roi (%) | **21.62%** | `(9000−7400)/7400 × 100 = 21.62%` | ✅ |
| destination_planet_name | ArcCorp (gleicher Planet) | Stanton | ⚠️ UEX zeigt präziseren Planeten |
| status_destination | **5** (hoher Bestand) | — | ✅ wertvoller Zusatz |

#### Beispiel 3: Waste — ARC-L1 → Pyro Gateway/Nyx (Inter-System)

| Feld | UEX-Wert | K1-(Rohdaten) | Differenz |
|---|---|---|---|
| Commodity | Waste (ID 79) | Waste (ID 79) | ✓ |
| Origin/Dest | ARC-L1 → PYROG (Nyx) | ARC-L1 → … | ✓ |
| price_origin | **229** | ~229 | ✅ kongruent |
| price_destination | **380** | variabel | ⚠️ UEX fixiert diesen Preis |
| profit | **1057** | `380−229 = 151` | ❌ UEX ≈ 7× K1 |
| margin (%) | **39.74%** | Gross-Margin | ✅ |
| roi (%) | **65.94%** | `(380−229)/229 × 100 = 65.94%` | ✅ Standard-Marge |
| distance | **92** | **3** (anderes System) | ❌ Total unterschiedlich |
| destination_star_system | **Nyx** | Stanton (K1 kennt kein Dest-System) | ✅ UEX detaillierter |
| destination_planet_name | **null** | — | Inter-System korrekt null |

**Profit-Analyse:** UEX profit ≠ K1 `sell_price − buy_price`. Der Faktor variiert stark (≈ 2× bei Laranite, ≈ 7× bei Waste). UEX nutzt eine nicht-transparente Berechnungsmetrik. ROI und Margin sind aber konsistent zwischen beiden Systemen.

---

## 4. Empfehlung: Option (b) — K1 als Primärquelle + UEX als Anreicherungsschicht

### Begründung

1. **Preisdatenkongruenz:** `price_origin` und `price_destination` stammen aus derselben Quelle wie K1 (`commodities_prices`). Beide Systeme zeigen konsistente Rohpreise.

2. **Optimierungsunterschied:** K1 findet das beste Buy→Sell-Paar über ALLE Terminals. UEX zeigt nur explizit abgefragte Kombinationen. Für die Kernfunktion ("beste Route finden") ist K1 überlegen.

3. **Score-Filterung:** UEX `score` ist ein vor-kalulierter Gesamtgütescore, der sich gut zum schnellen Sortieren eignet — aber nur wenn man bereits ein bestimmtes Origin/Dest-Paar betrachtet hat.

4. **Wertvolle Zusatzfelder:** 50+ neue Felder (Container-Größen, User-Preise, Fraktionen, Game-Version, 9× Terminal-Flags pro Seite) bieten echten Mehrwert, der lokal nicht existiert.

5. **Rate-Limit & Cache:** Mit stündlichem Update und 30-Min-Cache kann UEX nur als Anreicherung leben, nicht als Primärquelle. Unser sub-minuten-generierter K1-Cache ist aktuelldr.

6. **Keine Auth nötig:** Beide Endpunkte (UEX + K1 via commodities_prices) benötigen keinen Auth-Token — kein Risiko für Keys.

### Konkrete Auswirkungen

Felder zur K1-Matrix hinzuzufügen (pro Route nach UEX-Lookup):

- `uex_score` (int) — UEX-Ranking
- `container_sizes_origin` / `container_sizes_destination` (string/csv)
- `user_price_origin` / `user_price_destination` (int) — Community-Preise
- `faction_origin` / `faction_destination` (string/null)
- `game_version_origin` / `game_version_destination` (string)
- `has_quantum_marker_origin` / `has_quantum_marker_destination` (int)
- `scu_reachable` (int) — verfügbare Erreichbarkeit

### Rate-Limit-Risiko

Mit 120 req/min und stündlichem Refresh kann UEX nur als Ergänzungsschicht dienen. Unser interner subminutengenauer K1-Cache ist aktueller. UEX-Antworten müssen min-weise cachen (30 Min Revalidation), max. 2 req/sec durchsetzen.

---

## Anhang A: Field Count Summary

| Kategorie | UEX | K1 | Neu |
|---|---|---|---|
| Identifikation | 3 | 3 | 0 |
| Commodity | 2 | 2 | 0 |
| Origin-Terminal | 9 | 3 | 6 |
| Origin-Flags | 9 | 3 | 6 |
| Destination-Terminal | 9 | 3 | 6 |
| Destination-Flags | 9 | 3 | 6 |
| Location-IDs | 8 | 0 | 8 |
| Pricing | 11 | 4 | 7 |
| SCU | 8 | 1 | 7 |
| Inventory | 2 | 2 | 0 |
| Container/Meta | 6 | 0 | 6 |
| **Summe** | **76** | **~15** | **~50** |

---

## Anhang B: Sample-Dateien

| Sample | Filter | Records | Datei im Workspace |
|---|---|---|---|
| A | `id_terminal_origin=1` | 34 (20 Laranite, 14 Waste) | `samples/sample-a-terminal-origin-1.json` |
| B | `id_commodity=79` (Waste) | 500 (Max; letzter Record hand-truncated) | `samples/sample-b-idcommodity-79.json` |
| C (Live) | `id_terminal_origin=1, dest=45` | 1 (ARC-L1→HUR-L2, Laranite) | `samples/sample-arc-hur-corrected.json` |
| D (Live) | `id_commodity=47, origin=1` | 20 (alle UEX-Optionen von ARC-L1) | `samples/sample-commodity47-origin1.json` |
| E (Live) | `id_commodity=47` | 120 (Alle Laranite-Routen) | `samples/sample-laranite.json` |
| F (Live) | `id_terminal_origin=1, limit=3` | 3 (Waste-Probe mit Full Fields) | `samples/sample-terminal-origin.json` |
