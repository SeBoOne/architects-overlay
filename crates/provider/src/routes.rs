//! Handelsrouten (Karte t_128b7547): DTOs + reine Berechnung.
//!
//! Die Berechnung ist bewusst QUELLENUNABHÄNGIG in `provider` (Testbarkeit,
//! kein UEX-Typ leakt): die Rohdaten kommen als einfache Zeilen herein
//! (`RoutePriceRow`), die Filter/Sortierung als [`RouteFilters`], heraus
//! kommen fertige Routen (`RouteList`). Die eigentliche Datenbeschaffung
//! (UEX-Sweep über alle Commodity-Terminals) lebt in `uex-client`.
//!
//! Semantik der Preise (wie im Rest der App, Spielersicht):
//! * `buy_price`  = Preis, den der SPIELER am Terminal zahlt (Kauf) —
//!   niedrigster Wert = bester Einkauf.
//! * `sell_price` = Preis, den der SPIELER am Terminal erhält (Verkauf) —
//!   höchster Wert = bester Verkauf.
//!
//! Entfernung: UEX liefert KEINE Koordinaten (Fact-Check t_128b7547: weder
//! `/terminals` noch `/moons`/`/planets` tragen x/y/z). Die Distanz ist daher
//! eine bewusste HIERARCHIE-PROXY über die Terminal-Kette
//! ("System > Planet > Moon > Ort"): gleicher Ort = 0, gleicher Mond = 1,
//! gleicher Planet (verschiedener Mond/Ort) = 2, gleicher System
//! (verschiedener Planet) = 3, anderes System = 4. Dokumentiert im Report.

use serde::{Deserialize, Serialize};

/// Rohzeile: Preis eines Terminals für eine Ware (Spielersicht, siehe Modul-
/// Kommentar). Die Quelle (UEX-Sweep) legt diese Zeilen in den Cache.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoutePriceRow {
    pub commodity_id: u64,
    pub commodity: String,
    pub terminal_id: u64,
    pub buy_price: f64,
    pub sell_price: f64,
    /// SCU-Bestand, den das Terminal verkauft (`scu_sell_stock`) — der
    /// Arkanis-Bestandsfilter ist bewusst NICHT implementiert, der Bestand
    /// geht nur als Anzeige in das Routen-Detail ein.
    pub sell_stock: f64,
    /// Inventar-Level 0 = unbekannt/leer, 1..=7.
    pub buy_status: u8,
    pub sell_status: u8,
    pub quality: u16,
    pub updated: u64,
}

/// Terminal-Flags aus `/terminals` (nicht in der Entity-Tabelle — sie leben
/// nur im Routen-Datensatz).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RouteTerminalMeta {
    /// UEX `is_auto_load` — Autoload-Fähigkeit (Cargo-Grid-Ladung).
    pub autoload: bool,
    /// UEX `is_cargo_center` — Cargodeck vorhanden.
    pub cargo_center: bool,
    /// Bodenort (Stadt/Outpost/POI) — andernfalls Orbit-Station/L-Point.
    pub ground: bool,
    /// Hierarchy chain "System > Planet > Mond > Ort" für Distanz/Filter.
    pub chain: String,
}

/// Ware-Flags aus `/commodities`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RouteCommodityMeta {
    pub illegal: bool,
    pub sellable: bool,
    pub buyable: bool,
}

/// Sortierung der Routenliste.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteSort {
    /// Standard: Gewinn pro SCU absteigend.
    #[default]
    ProfitDesc,
    ProfitAsc,
    /// Distanz (Hierarchie-Proxy) aufsteigend.
    DistanceAsc,
    /// Absoluter Gewinn (profit_per_scu × SCU) absteigend.
    TotalProfitDesc,
    /// Marge (Gewinn / Einkaufskosten) absteigend.
    MarginDesc,
    /// Neutral: keine explizite Sortierung — Originalliste (Provider-Reihenfolge)
    /// unverändert. Deckt den neutralen Zustand des dynamischen Gewinn/SCU-Schalters.
    Natural,
}

impl RouteSort {
    pub fn as_str(&self) -> &'static str {
        match self {
            RouteSort::ProfitDesc => "profit_desc",
            RouteSort::ProfitAsc => "profit_asc",
            RouteSort::DistanceAsc => "distance_asc",
            RouteSort::TotalProfitDesc => "total_profit_desc",
            RouteSort::MarginDesc => "margin_desc",
            RouteSort::Natural => "natural",
        }
    }
}

/// Legality-Filter: 0 = alle, 1 = nur legale, 2 = nur illegale Waren.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteLegality {
    #[default]
    All,
    Legal,
    Illegal,
}

/// Filter (Kartenvorgabe): Ware, Startort, Zielort, Legal/Illegal, Autoload,
/// Cargodeck, nur Boden-/nur Space-Orte, SCU-Kapazität. BEWUSST ohne
/// Mindest-/Maximalbestand (Unterschied zu Arkanis).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)] // IPC: fehlende Felder = Default (Frontend sendet schmal)
pub struct RouteFilters {
    /// Ware einschränken (Name-Substring, case-insensitive).
    pub commodity: Option<String>,
    /// Startort einschränken (Terminal-Name ODER Kette, Substring).
    pub from: Option<String>,
    /// Zielort einschränken (Terminal-Name ODER Kette, Substring).
    pub to: Option<String>,
    pub legality: RouteLegality,
    pub autoload: Option<bool>,
    pub cargo_deck: Option<bool>,
    pub ground_only: bool,
    pub space_only: bool,
    /// SCU-Kapazität (Schiff aus Hangar oder manuell) für den Absolutgewinn.
    pub scu: f64,
    /// Budget (aUEC) — RB2 (Karte t_9b42249a). Wenn gesetzt (Some(>0)),
    /// wird die kaufbare Menge pro Route auf
    /// min(Schiff-SCU, floor(Budget / Kaufpreis pro SCU)) gedeckelt und
    /// `profit_total`/`cost_total` entsprechend neu gerechnet. None oder
    /// 0 = Vollkapazität (Verhalten wie heute).
    pub budget: Option<f64>,
    pub sort: RouteSort,
}

/// RB2: kaufbare Menge = min(freier Schiffscapacity in SCU, floor(Budget /
/// Kaufpreis pro SCU)). Ohne Budget (None/≤0) = volle Kapazität; Kaufpreis
/// ≤ 0 kann nicht bezahlt werden → Menge 0.
pub fn buyable_quantity(ship_scu: f64, budget: Option<f64>, buy_price: f64) -> f64 {
    let capacity = ship_scu.max(0.0);
    match budget {
        Some(b) if b > 0.0 => {
            if buy_price <= 0.0 {
                return 0.0;
            }
            let affordable = (b / buy_price).floor();
            capacity.min(affordable)
        }
        // None oder ≤ 0: kein Budget gesetzt → Vollkapazität wie heute.
        _ => capacity,
    }
}

/// UEX-Enrichment (Integrations-Entscheidung Option b, t_ce154aa5): die lokale
/// Routen-Matrix (K1) bleibt OWNER der Profit-/Sortier-Logik — vom UEX
/// `/commodities_routes`-Response werden NUR die ergänzenden Felder übernommen
/// (Volumen/Nachfrage/Scarcity/Zeitstempel + Terminal-/Fraktions-/Score-Meta)
/// und den Routen-Zeilen als Zusatzinfo beigemischt. UEX-eigene Profit-/Marge-/
/// ROI-Felder sind BEWUSST NICHT enthalten — die Anzeige/Sortierung rechnet
/// weiter rein aus `sell_price − buy_price` (K1). Ist das Enrichment fehlend
/// (UEX-Ausfall, Cache leer), bleibt die Route voll funktionsfähig.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RouteEnrichment {
    /// `score` — UEX-Gesamtgüte (nur Anzeige, KEINE Sortierung).
    pub uex_score: i64,
    /// `scu_origin` — verfügbare SCU am Einkaufsterminal (Volumen).
    pub scu_origin: f64,
    /// `scu_destination` — verfügbare SCU am Verkaufsterminal (Volumen).
    pub scu_destination: f64,
    /// `scu_reachable` — mit typischem Frachter erreichbare SCU (Volumen).
    pub scu_reachable: f64,
    /// `status_origin` bzw. `status_destination` — Lagerbestand 0..=7
    /// (Scarcity/Nachfrage-Anzeige, nicht Filter).
    pub status_origin: u8,
    pub status_destination: u8,
    /// `container_sizes_*` — CSV der nutzbaren Container (Origin/Destination).
    pub container_sizes_origin: String,
    pub container_sizes_destination: String,
    /// `price_*_users_rows` — Anzahl Community-Preismeldungen (Nachfragetiefe).
    pub user_price_origin_rows: u64,
    pub user_price_dest_rows: u64,
    /// `game_version_*` — Spielversion des Preisdatensatzes (gamedou geflickt).
    pub game_version_origin: Option<String>,
    pub game_version_destination: Option<String>,
    /// `*_faction_name` — Fraktionsname (null bei Inter-System-Zielen).
    pub faction_origin: Option<String>,
    pub faction_destination: Option<String>,
    /// `has_quantum_marker_*` — Quantenmarker vorhanden.
    pub has_quantum_marker_origin: bool,
    pub has_quantum_marker_destination: bool,
    /// `date_added` — Unix-Timestamp der Routen-Erfassung.
    pub date_added: u64,
}

/// Eine fertige Route: bester Einkauf → bester Verkauf einer Ware. Das optionale
/// UEX-Enrichment (Option b) hängt als Zusatzinfo dran — es ist NIE
/// sortierungs- oder profitrelevant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeRoute {
    pub commodity_id: u64,
    pub commodity: String,
    pub illegal: bool,
    // --- Kauf-Seite ---
    pub buy_terminal_id: u64,
    pub buy_terminal: String,
    pub buy_chain: String,
    pub buy_price: f64,
    pub buy_terminal_autoload: bool,
    pub buy_terminal_cargo_deck: bool,
    pub buy_terminal_ground: bool,
    // --- Verkauf-Seite ---
    pub sell_terminal_id: u64,
    pub sell_terminal: String,
    pub sell_chain: String,
    pub sell_price: f64,
    pub sell_terminal_autoload: bool,
    pub sell_terminal_cargo_deck: bool,
    pub sell_terminal_ground: bool,
    /// Verfügbare SCU am Verkaufsterminal (Anzeige).
    pub sell_stock: f64,
    /// Gewinn pro SCU = sell_price − buy_price (> 0 garantiert).
    pub profit_per_scu: f64,
    /// Kaufbare Menge in SCU für die aktuelle Abfrage: ohne Budget = volle
    /// Schiffscapacity (`filters.scu`), mit Budget = min(SCU,
    /// floor(Budget / buy_price)). RB2 — die Menge, mit der `profit_total`
    /// und `cost_total` gerechnet werden; Menge 0 = mit dem Budget ist
    /// kein einziger SCU kaufbar (Route ausgegraut/sortiert ans Ende).
    #[serde(default)]
    pub quantity: f64,
    /// Absolutgewinn = profit_per_scu × quantity (budget-capped).
    pub profit_total: f64,
    /// Einkaufskosten = buy_price × quantity (budget-capped).
    pub cost_total: f64,
    /// Marge = profit_per_scu / buy_price (0..∞), gerundet auf 4 Stellen.
    pub margin: f64,
    /// Entfernung als Hierarchie-Proxy 0..=4 (siehe Modulkommentar).
    pub distance: u8,
    /// Optionaler UEX-Enrichment-Block (Volumen/Nachfrage/Meta). Wird beim
    /// Servieren aus dem `route_enrichment`-Cache beigemischt; wenn der Cache
    /// leer/UEX down ist, ist es `None` und die Route bleibt voll lauffähig.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrichment: Option<RouteEnrichment>,
}

/// Ergebnis der Berechnung: Routen (bereits gefiltert + sortiert) plus
/// Metadaten für die UI (Datenstand).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteList {
    pub routes: Vec<TradeRoute>,
    /// Unix-Sekunden des letzten Sweeps (Datenstand-Anzeige).
    pub swept_at: u64,
    /// Stale-while-revalidate: `true` when the sweep marker is older than
    /// the Sweep-TTL — the served data is (possibly) veralt and a
    /// background refresh runs/runs next. UI shows the ↻ marker.
    #[serde(default)]
    pub refreshing: bool,
}

/// Eine Kauf- ODER Verkaufsstelle im Alternativ-Tabellen-Beleg (F2): das
/// Terminal, seine Hierarchie-Kette (System > Planet > ...), der Preis je SCU
/// in dieser Richtung und die lokalen Bestands-/Nachfrage-Werte. Wird aus den
/// bereits im Cache liegenden `RoutePriceRow`s abgeleitet — KEIN neuer
/// UEX-Call (Datenbasis = bestehende lokale Matrix/Enrichment).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteLocation {
    pub terminal_id: u64,
    pub terminal: String,
    pub chain: String,
    /// Kauf-preis (linke Spalte) bzw. Verkaufspreis (rechts) je SCU.
    pub price: f64,
    /// Verfügbare SCU am Terminal (`sell_stock`).
    pub stock: f64,
    /// Lager-Nachfrage 0..=7 (kauf: buy_status, verkauf: sell_status).
    pub status: u8,
    /// Preisstand dieser Zeile (UEX `date_modified`, Unix-Sekunden). 0 =
    /// unbekannt. Das Frontend zeigt einen Alters-Chip für alte Zeilen
    /// (Bug t_15e598e7: verwaiste/veraltete Zeilen ohne Kennzeichnung).
    pub updated: u64,
    pub autoload: bool,
    pub cargo_deck: bool,
    pub ground: bool,
}

/// Zweispaltige Alternativ-Tabelle für ein Commodity (F2): alle Kauforte
/// (Preis aufsteigend) links, alle Verkaufsorte (Preis absteigend) rechts —
/// abgeleitet aus den lokalen Routen-Zeilen, sortiert im Frontend nach Preis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteCommodityLocations {
    pub commodity_id: u64,
    pub commodity: String,
    /// Alle Terminals, an denen das Commodity gekauft wird (buy_price > 0).
    pub buys: Vec<RouteLocation>,
    /// Alle Terminals, an denen es verkauft wird (sell_price > 0).
    pub sells: Vec<RouteLocation>,
}

/// Baut aus den lokalen Routen-Rohzeilen die Kauf-/Verkaufsort-Tabelle einer
/// Ware. Nur Preise > 0 gelten als Ort; ein Terminal kann in beiden Listen
/// auftauchen (kauft UND verkauft die Ware).
///
/// Terminal-Typ-Whitelist (BUG t_5a6706f7): `terminal_flags` enthält nur
/// Terminals mit Routen-Typ ([`crate::routes`-Doku], P15a — `commodity`/
/// `commodity_raw`). Zeilen von Terminals OHNE Flags-Eintrag (Item-Shops,
/// Raffinerie-Item-Teile, Legacy-/Geisterzeilen) sind KEINE Commodity-Handels-
/// Orte und werden hier — anders als vor diesem Fix — verworfen. Die
/// Routen-Matrix hat diesen Skip schon immer gehabt (siehe
/// [`compute_routes_full`]); nur die Alternativ-Tabelle ließ die Zeilen durch.
/// Nachfrage-/Bestandswerte kommen
/// aus den Zeilen (`buy_status`/`sell_status`/`sell_stock`) — der lokale
/// Enrichment-Stand, kein Netzwerkzugriff.
pub fn route_commodity_locations(
    rows: &[RoutePriceRow],
    terminal_flags: &std::collections::HashMap<u64, RouteTerminalMeta>,
    terminal_names: &std::collections::HashMap<u64, String>,
    commodity_id: u64,
) -> Option<RouteCommodityLocations> {
    let name = |id: u64| -> String {
        terminal_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("Terminal {id}"))
    };
    let empty = RouteTerminalMeta::default();
    let row_at = |r: &RoutePriceRow, buy: bool| RouteLocation {
        terminal_id: r.terminal_id,
        terminal: name(r.terminal_id),
        chain: terminal_flags
            .get(&r.terminal_id)
            .unwrap_or(&empty)
            .chain
            .clone(),
        price: if buy { r.buy_price } else { r.sell_price },
        stock: r.sell_stock,
        status: if buy { r.buy_status } else { r.sell_status },
        updated: r.updated,
        autoload: terminal_flags
            .get(&r.terminal_id)
            .unwrap_or(&empty)
            .autoload,
        cargo_deck: terminal_flags
            .get(&r.terminal_id)
            .unwrap_or(&empty)
            .cargo_center,
        ground: terminal_flags.get(&r.terminal_id).unwrap_or(&empty).ground,
    };

    let commodity_name = rows
        .iter()
        .find(|r| r.commodity_id == commodity_id)
        .map(|r| r.commodity.clone());

    if commodity_name.is_none() {
        return None;
    }

    // Dedupe pro Terminal (der Sweep kann mehrere Zeilen je Ware liefern):
    // bester KAUF = niedrigster buy_price, bester VERKAUF = höchster
    // sell_price je Terminal.
    let best_per_terminal = |key: fn(&RoutePriceRow) -> f64,
                             maximize: bool|
     -> std::collections::HashMap<u64, &RoutePriceRow> {
        let mut acc: std::collections::HashMap<u64, &RoutePriceRow> =
            std::collections::HashMap::new();
        for r in rows
            .iter()
            .filter(|r| r.commodity_id == commodity_id)
            // Terminal-Typ-Whitelist (BUG t_5a6706f7): Zeilen von Terminals
            // ohne Route-Flags (Item-Shops, Raffinerie-Item-Teile, Geister-
            // zeilen aus der Legacy-Ära) sind keine Commodity-Handelsorte und
            // dürfen nicht in die Tabelle. `terminal_flags` trägt nur
            // `is_route_terminal_type`-Terminals — Flags-Mitgliedschaft IST
            // der Typ-Filter. Die Routen-Matrix ([`compute_routes_full`])
            // skipped flag-lose Terminals schon immer; nur die Alternativ-
            // Tabelle ließ sie bisher durch.
            .filter(|r| terminal_flags.contains_key(&r.terminal_id))
        {
            acc.entry(r.terminal_id)
                .and_modify(|best| {
                    let better = if maximize {
                        key(r) > key(best)
                    } else {
                        key(r) < key(best)
                    };
                    if better {
                        *best = r;
                    }
                })
                .or_insert(r);
        }
        acc
    };
    let buys: Vec<RouteLocation> = best_per_terminal(|r| r.buy_price, false)
        .into_values()
        .filter(|r| r.buy_price > 0.0)
        .map(|r| row_at(r, true))
        .collect();
    let sells: Vec<RouteLocation> = best_per_terminal(|r| r.sell_price, true)
        .into_values()
        .filter(|r| r.sell_price > 0.0)
        .map(|r| row_at(r, false))
        .collect();

    Some(RouteCommodityLocations {
        commodity_id,
        commodity: commodity_name.unwrap(),
        buys,
        sells,
    })
}

/// Does this query use terminal-SIDE filters (Start/Target/Autoload/Cargo-
/// Deck/Boden/Space)? If not, the persisted Profit-Matrix can serve the query
/// directly; if yes, the rows behind the matrix must be re-scanned (the
/// filter changes which Buy/Sell pair is „best“ per Ware).
impl RouteFilters {
    pub fn has_terminal_side_filters(&self) -> bool {
        self.from.as_deref().filter(|f| !f.is_empty()).is_some()
            || self.to.as_deref().filter(|f| !f.is_empty()).is_some()
            || self.autoload.is_some()
            || self.cargo_deck.is_some()
            || self.ground_only
            || self.space_only
    }
}

/// Serve a query FROM the persisted Profit-Matrix (no re-scan of the price
/// rows): Matrix-rows are already the best Buy→Sell pair per Commodity.
/// Only commodity-Substring- and Legality-Filter apply on this lane —
/// guarded by the caller ([`RouteFilters::has_terminal_side_filters`]).
/// `profit_total`/`cost_total` are applied from the stored per-SCU values
/// (identical formulas to [`compute_routes_full`]).
pub fn routes_from_matrix(matrix: &[TradeRoute], filters: &RouteFilters) -> Vec<TradeRoute> {
    let scu = filters.scu.max(0.0);
    let mut out: Vec<TradeRoute> = matrix
        .iter()
        .filter(
            |r| match filters.commodity.as_deref().filter(|f| !f.is_empty()) {
                Some(f) => {
                    let f = f.to_lowercase();
                    r.commodity.to_lowercase().contains(&f) || r.commodity_id.to_string() == f
                }
                None => true,
            },
        )
        .filter(|r| match filters.legality {
            RouteLegality::All => true,
            RouteLegality::Legal => !r.illegal,
            RouteLegality::Illegal => r.illegal,
        })
        .map(|r| {
            let mut out = r.clone();
            let qty = buyable_quantity(scu, filters.budget, r.buy_price);
            out.quantity = qty;
            out.profit_total = r.profit_per_scu * qty;
            out.cost_total = r.buy_price * qty;
            out
        })
        .collect();
    sort_routes(
        &mut out,
        filters.sort,
        filters.budget.map_or(false, |b| b > 0.0),
    );
    out
}

/// Filtert + sortiert die Rohzeilen zu Routen. Eine Route entsteht pro Ware
/// aus dem GÜNSTIGSTEN Einkauf und dem BESTEN Verkauf an einem ANDEREN
/// Terminal, sofern beide Richtungen existieren und der Gewinn positiv ist.
///
/// Bestandsgrenzen (Min-/Max-Bestand) sind bewusst NICHT Teil der Filter —
/// genannter Unterschied zu Arkanis (Karte).
/// Vollständige Berechnung: Rohzeilen + Terminal-Flags/-Namen + Ware-Flags →
/// gefilterte, sortierte Routen.
pub fn compute_routes_full(
    rows: &[RoutePriceRow],
    terminal_flags: &std::collections::HashMap<u64, RouteTerminalMeta>,
    terminal_names: &std::collections::HashMap<u64, String>,
    commodity_flags: &std::collections::HashMap<u64, RouteCommodityMeta>,
    filters: &RouteFilters,
) -> Vec<TradeRoute> {
    let scu = filters.scu.max(0.0);

    // (commodity_id → Terminal-Zeilen). Nur Zeilen mit Nutzpreis.
    let mut by_commodity: std::collections::HashMap<u64, Vec<&RoutePriceRow>> =
        std::collections::HashMap::new();
    for r in rows {
        let Some(meta) = terminal_flags.get(&r.terminal_id) else {
            continue; // Terminal ohne Metadaten (frisch importiert) — skippen
        };
        // Terminal-Filter: Autoload / Cargodeck / Boden-Space.
        if filters.autoload.is_some_and(|want| meta.autoload != want) {
            continue;
        }
        if filters
            .cargo_deck
            .is_some_and(|want| meta.cargo_center != want)
        {
            continue;
        }
        if filters.ground_only && !meta.ground {
            continue;
        }
        if filters.space_only && meta.ground {
            continue;
        }
        // Start-/Zielort-Filter greifen SEITENBEZOGEN auf den Kandidaten
        // (unten): `from` auf die Kaufseite, `to` auf die Verkaufsseite.
        if r.buy_price > 0.0 || r.sell_price > 0.0 {
            by_commodity.entry(r.commodity_id).or_default().push(r);
        }
    }

    let commodity_match = |name: &str, id: u64| -> bool {
        match filters.commodity.as_deref().filter(|f| !f.is_empty()) {
            Some(f) => name.to_lowercase().contains(&f.to_lowercase()) || id.to_string() == f,
            None => true,
        }
    };

    let mut out: Vec<TradeRoute> = Vec::new();
    for (_id, group) in by_commodity {
        // Kauf-Kandidaten (Spieler kauft dort → niedrigster buy_price), mit
        // FROM-Filter auf der Kaufseite.
        let from_f = filters.from.as_deref().filter(|f| !f.is_empty());
        let buys = group
            .iter()
            .filter(|r| {
                r.buy_price > 0.0 && {
                    match from_f {
                        Some(f) => {
                            let f = f.to_lowercase();
                            let name = terminal_names
                                .get(&r.terminal_id)
                                .map(String::as_str)
                                .unwrap_or("");
                            let chain = terminal_flags
                                .get(&r.terminal_id)
                                .map(|m| m.chain.as_str())
                                .unwrap_or("");
                            format!("{name} {chain}").to_lowercase().contains(&f)
                        }
                        None => true,
                    }
                }
            })
            .copied()
            .min_by(|a, b| a.buy_price.total_cmp(&b.buy_price));
        // Verkauf-Kandidaten (Spieler verkauft dort → höchster sell_price),
        // mit TO-Filter auf der Verkaufseite.
        let to_f = filters.to.as_deref().filter(|f| !f.is_empty());
        let sells = group
            .iter()
            .filter(|r| {
                r.sell_price > 0.0 && {
                    match to_f {
                        Some(f) => {
                            let f = f.to_lowercase();
                            let meta_name = terminal_names
                                .get(&r.terminal_id)
                                .map(String::as_str)
                                .unwrap_or("");
                            let hay = format!(
                                "{meta_name} {}",
                                terminal_flags
                                    .get(&r.terminal_id)
                                    .map(|m| m.chain.as_str())
                                    .unwrap_or("")
                            )
                            .to_lowercase();
                            hay.contains(&f)
                        }
                        None => true,
                    }
                }
            })
            .copied()
            .max_by(|a, b| a.sell_price.total_cmp(&b.sell_price));
        let (Some(buy), Some(sell)) = (buys, sells) else {
            continue;
        };
        if buy.terminal_id == sell.terminal_id {
            continue;
        }
        let profit = sell.sell_price - buy.buy_price;
        if profit <= 0.0 {
            continue;
        }
        // Fehlende Ware-Metadaten = legal (Konservativ-Default): der Sweep
        // liefert Flags für das volle /commodities-Dataset, nur beim
        // Unit-Test mit leerer Map bleibt der Default greifbar.
        let empty_commodity = RouteCommodityMeta::default();
        let cmeta = commodity_flags
            .get(&buy.commodity_id)
            .unwrap_or(&empty_commodity);
        // Legality-Filter.
        match filters.legality {
            RouteLegality::All => {}
            RouteLegality::Legal => {
                if cmeta.illegal {
                    continue;
                }
            }
            RouteLegality::Illegal => {
                if !cmeta.illegal {
                    continue;
                }
            }
        }
        let name = buy.commodity.clone();
        if !commodity_match(&name, buy.commodity_id) {
            continue;
        }
        let empty = RouteTerminalMeta::default();
        let b = terminal_flags.get(&buy.terminal_id).unwrap_or(&empty);
        let s = terminal_flags.get(&sell.terminal_id).unwrap_or(&empty);
        let fallback_name = |id: u64| -> String {
            terminal_names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| format!("Terminal {id}"))
        };
        // RB2: kaufbare Menge = min(SCU, floor(Budget/buy_price)); ohne
        // Budget = volle Kapazität. Profit/Kosten mit derselben Menge.
        let qty = buyable_quantity(scu, filters.budget, buy.buy_price);
        out.push(TradeRoute {
            commodity_id: buy.commodity_id,
            commodity: name,
            illegal: cmeta.illegal,
            buy_terminal_id: buy.terminal_id,
            buy_terminal: fallback_name(buy.terminal_id),
            buy_chain: b.chain.clone(),
            buy_price: buy.buy_price,
            buy_terminal_autoload: b.autoload,
            buy_terminal_cargo_deck: b.cargo_center,
            buy_terminal_ground: b.ground,
            sell_terminal_id: sell.terminal_id,
            sell_terminal: fallback_name(sell.terminal_id),
            sell_chain: s.chain.clone(),
            sell_price: sell.sell_price,
            sell_terminal_autoload: s.autoload,
            sell_terminal_cargo_deck: s.cargo_center,
            sell_terminal_ground: s.ground,
            sell_stock: sell.sell_stock,
            profit_per_scu: profit,
            // RB2: kaufbare Menge = min(SCU, floor(Budget/buy_price)); ohne
            // Budget = volle Kapazität. Profit/Kosten mit derselben Menge.
            quantity: qty,
            profit_total: profit * qty,
            cost_total: buy.buy_price * qty,
            margin: if buy.buy_price > 0.0 {
                (profit / buy.buy_price * 1000.0).round() / 1000.0
            } else {
                0.0
            },
            distance: hierarchy_distance(&b.chain, &s.chain),
            enrichment: None,
        });
    }

    sort_routes(
        &mut out,
        filters.sort,
        filters.budget.map_or(false, |b| b > 0.0),
    );
    out
}

/// Sortiert in place nach der gewählten Reihenfolge.
///
/// RB2: Ist ein Budget gesetzt (ein/mehrere Routen mit Menge 0 — mit dem
/// Budget ist nicht einmal 1 SCU kaufbar), sinken diese Routen RANGLOS ans
/// Ende der Liste (unabhängig von der Sortierwahl); sie werden im UI
/// ausgegraut. Ohne Budget bleibt das Verhalten wie heute (keine
/// Menge-Sonderbehandlung).
fn sort_routes(routes: &mut [TradeRoute], sort: RouteSort, budget_active: bool) {
    // Budget-Versenkung: bei gesetztem Budget sinkt jede Route mit Menge ≤ 0
    // (nicht einmal 1 SCU kaufbar) RANGLOS ans Ende — der Menge-0-Flag als
    // Primary-Key (false=oben < true=unten), danach die eigentliche Sortierung.
    fn zero_rank(budget_active: bool, r: &TradeRoute) -> u8 {
        if budget_active && r.quantity <= 0.0 {
            1
        } else {
            0
        }
    }
    let z = |r: &TradeRoute| zero_rank(budget_active, r);
    let cmp_rank = |a: &TradeRoute, b: &TradeRoute| z(a).cmp(&z(b));
    match sort {
        RouteSort::ProfitDesc => routes.sort_by(|a, b| {
            cmp_rank(a, b).then_with(|| {
                b.profit_per_scu
                    .total_cmp(&a.profit_per_scu)
                    .then_with(|| a.commodity.cmp(&b.commodity))
            })
        }),
        RouteSort::ProfitAsc => routes.sort_by(|a, b| {
            cmp_rank(a, b).then_with(|| {
                a.profit_per_scu
                    .total_cmp(&b.profit_per_scu)
                    .then_with(|| a.commodity.cmp(&b.commodity))
            })
        }),
        RouteSort::DistanceAsc => routes.sort_by(|a, b| {
            cmp_rank(a, b).then_with(|| {
                a.distance
                    .cmp(&b.distance)
                    .then_with(|| b.profit_per_scu.total_cmp(&a.profit_per_scu))
            })
        }),
        RouteSort::TotalProfitDesc => routes
            .sort_by(|a, b| cmp_rank(a, b).then_with(|| b.profit_total.total_cmp(&a.profit_total))),
        RouteSort::MarginDesc => {
            routes.sort_by(|a, b| cmp_rank(a, b).then_with(|| b.margin.total_cmp(&a.margin)))
        }
        RouteSort::Natural => {} // Originalliste (keine explizite Sortierung)
    }
}

/// Hierarchie-Distanz zweier Terminal-Ketten (0..=4, siehe Modulkommentar).
pub fn hierarchy_distance(a: &str, b: &str) -> u8 {
    if a == b {
        return 0;
    }
    fn segs(c: &str) -> Vec<&str> {
        c.split('>')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect()
    }
    let (sa, sb) = (segs(a), segs(b));
    // Gemeinsames Präfix zählt: System gleich?
    if sa.first() != sb.first() {
        return 4; // anderes Sternsystem
    }
    // gleicher Ort: Kette endet identisch im LETZTEN Segment (Ort) und der
    // Mond stimmt überein → gleicher Ort/gleicher Mond-Level.
    if sa == sb {
        return 0;
    }
    let moon_a = sa.get(2);
    let moon_b = sb.get(2);
    let planet_a = sa.get(1);
    let planet_b = sb.get(1);
    if moon_a.is_some() && moon_a == moon_b {
        return 1; // gleicher Mond
    }
    if planet_a.is_some() && planet_a == planet_b {
        return 2; // gleicher Planet
    }
    3 // gleicher Sternsystem, verschiedener Planet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(autoload: bool, cargo: bool, ground: bool, chain: &str) -> RouteTerminalMeta {
        RouteTerminalMeta {
            autoload,
            cargo_center: cargo,
            ground,
            chain: chain.to_string(),
        }
    }

    fn row(id: u64, term: u64, name: &str, buy: f64, sell: f64) -> RoutePriceRow {
        RoutePriceRow {
            commodity_id: id,
            commodity: name.to_string(),
            terminal_id: term,
            buy_price: buy,
            sell_price: sell,
            sell_stock: 0.0,
            buy_status: 0,
            sell_status: 0,
            quality: 0,
            updated: 0,
        }
    }

    fn meta_map(
        entries: &[(u64, RouteTerminalMeta)],
    ) -> std::collections::HashMap<u64, RouteTerminalMeta> {
        entries.iter().cloned().collect()
    }

    fn names_of(
        t: &std::collections::HashMap<u64, RouteTerminalMeta>,
    ) -> std::collections::HashMap<u64, String> {
        // Test-Chain "S > P > M > Ort" → letztes Segment als Terminalname.
        t.iter()
            .map(|(id, m)| {
                let name = m.chain.rsplit('>').next().unwrap_or("").trim();
                (*id, format!("Terminal - {name}"))
            })
            .collect()
    }

    fn compute(
        rows: &[RoutePriceRow],
        t: &std::collections::HashMap<u64, RouteTerminalMeta>,
        c: &std::collections::HashMap<u64, RouteCommodityMeta>,
        f: &RouteFilters,
    ) -> Vec<TradeRoute> {
        compute_routes_full(rows, t, &names_of(t), c, f)
    }

    fn base_filters() -> RouteFilters {
        RouteFilters {
            scu: 100.0,
            sort: RouteSort::ProfitDesc,
            ..Default::default()
        }
    }

    #[test]
    fn computes_best_buy_sell_pair_per_commodity() {
        let rows = vec![
            row(1, 10, "Laranite", 100.0, 0.0),
            row(1, 20, "Laranite", 90.0, 0.0), // bester Einkauf
            row(1, 30, "Laranite", 0.0, 150.0),
            row(1, 40, "Laranite", 0.0, 160.0), // bester Verkauf
        ];
        let t = meta_map(&[
            (
                10,
                flags(
                    false,
                    false,
                    true,
                    "Stanton > Crusader > Daymar > Outpost A",
                ),
            ),
            (
                20,
                flags(
                    false,
                    false,
                    true,
                    "Stanton > Crusader > Daymar > Outpost B",
                ),
            ),
            (
                30,
                flags(false, false, true, "Stanton > Crusader > Yela > Grim HEX"),
            ),
            (
                40,
                flags(false, false, true, "Stanton > MicroTech > Port Tressler"),
            ),
        ]);
        let routes = compute(&rows, &t, &Default::default(), &base_filters());
        assert_eq!(routes.len(), 1);
        let r = &routes[0];
        assert_eq!(r.buy_terminal_id, 20);
        assert_eq!(r.sell_terminal_id, 40);
        assert!((r.profit_per_scu - 70.0).abs() < 1e-9);
        assert!((r.profit_total - 7000.0).abs() < 1e-9);
        assert_eq!(r.distance, 3); // Crusader → MicroTech = anderer Planet
    }

    #[test]
    fn negative_or_zero_gap_is_not_a_route() {
        let rows = vec![
            row(1, 10, "Laranite", 200.0, 0.0),
            row(1, 20, "Laranite", 0.0, 150.0),
        ];
        let t = meta_map(&[
            (
                10,
                flags(false, false, true, "Stanton > Crusader > Yela > A"),
            ),
            (
                20,
                flags(false, false, true, "Stanton > Crusader > Yela > B"),
            ),
        ]);
        assert!(compute(&rows, &t, &Default::default(), &base_filters()).is_empty());
    }

    #[test]
    fn same_terminal_is_never_a_route() {
        let rows = vec![row(1, 10, "Laranite", 100.0, 150.0)];
        let t = meta_map(&[(
            10,
            flags(false, false, true, "Stanton > Crusader > Yela > A"),
        )]);
        assert!(compute(&rows, &t, &Default::default(), &base_filters()).is_empty());
    }

    #[test]
    fn legality_filter_selects_side() {
        let mut cflags = std::collections::HashMap::new();
        cflags.insert(
            1u64,
            RouteCommodityMeta {
                illegal: true,
                sellable: true,
                buyable: true,
            },
        );
        let rows = vec![
            row(1, 10, "Widow", 100.0, 0.0),
            row(1, 20, "Widow", 0.0, 200.0),
        ];
        let t = meta_map(&[
            (
                10,
                flags(false, false, true, "Stanton > Crusader > Yela > A"),
            ),
            (
                20,
                flags(false, false, true, "Stanton > Crusader > Yela > B"),
            ),
        ]);
        let mut f = base_filters();
        assert_eq!(compute(&rows, &t, &cflags, &f).len(), 1); // All
        f.legality = RouteLegality::Legal;
        assert!(compute(&rows, &t, &cflags, &f).is_empty());
        f.legality = RouteLegality::Illegal;
        assert_eq!(compute(&rows, &t, &cflags, &f).len(), 1);
    }

    #[test]
    fn terminal_flag_filters_apply() {
        let rows = vec![
            row(1, 10, "Agricium", 100.0, 0.0),
            row(1, 20, "Agricium", 0.0, 200.0),
        ];
        let t = meta_map(&[
            (10, flags(true, true, true, "Stanton > Crusader > Yela > A")),
            (
                20,
                flags(false, false, false, "Stanton > Crusader > Yela > B"),
            ),
        ]);
        let mut f = base_filters();
        f.autoload = Some(true);
        // Kauf-Terminal 10 ist Autoload-fähig, Verkauf 20 nicht → mit dem
        // Filter auf BEIDEN Seiten fällt die Route weg.
        assert!(compute(&rows, &t, &Default::default(), &f).is_empty());
        f.autoload = None;
        f.ground_only = true;
        assert!(compute(&rows, &t, &Default::default(), &f).is_empty()); // 20 ist Space
        f.ground_only = false;
        f.space_only = true;
        assert!(compute(&rows, &t, &Default::default(), &f).is_empty()); // 10 ist Boden
        f.space_only = false;
        assert_eq!(compute(&rows, &t, &Default::default(), &f).len(), 1);
    }

    #[test]
    fn from_to_filters_match_name_and_chain() {
        let rows = vec![
            row(1, 10, "Agricium", 100.0, 0.0),
            row(1, 20, "Agricium", 0.0, 200.0),
        ];
        let t = meta_map(&[
            (
                10,
                flags(false, false, true, "Stanton > Crusader > Yela > Grim HEX"),
            ),
            (
                20,
                flags(false, false, true, "Stanton > Hurston > Arial > Lorville"),
            ),
        ]);
        let mut f = base_filters();
        f.from = Some("Grim".to_string());
        assert_eq!(compute(&rows, &t, &Default::default(), &f).len(), 1);
        f.from = Some("Everus".to_string());
        assert!(compute(&rows, &t, &Default::default(), &f).is_empty());
        f.from = None;
        f.to = Some("hurston".to_string());
        assert_eq!(compute(&rows, &t, &Default::default(), &f).len(), 1);
        f.to = Some("yela".to_string());
        assert!(compute(&rows, &t, &Default::default(), &f).is_empty());
    }

    #[test]
    fn commodity_substring_filter() {
        let rows = vec![
            row(1, 10, "Laranite", 100.0, 0.0),
            row(2, 20, "Titanium", 50.0, 0.0),
            row(1, 30, "Laranite", 0.0, 150.0),
            row(2, 40, "Titanium", 0.0, 80.0),
        ];
        let t = meta_map(&[
            (
                10,
                flags(false, false, true, "Stanton > Crusader > Yela > A"),
            ),
            (
                20,
                flags(false, false, true, "Stanton > Crusader > Yela > A"),
            ),
            (
                30,
                flags(false, false, true, "Stanton > Hurston > Arial > B"),
            ),
            (
                40,
                flags(false, false, true, "Stanton > Hurston > Arial > B"),
            ),
        ]);
        let mut f = base_filters();
        f.commodity = Some("laran".to_string());
        let routes = compute(&rows, &t, &Default::default(), &f);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].commodity, "Laranite");
    }

    #[test]
    fn sort_orders() {
        let rows = vec![
            row(1, 10, "Cheap", 10.0, 0.0), // profit 10, margin 100%
            row(1, 20, "Cheap", 0.0, 20.0),
            row(2, 10, "Rich", 100.0, 0.0), // profit 110, margin 110%
            row(2, 20, "Rich", 0.0, 210.0),
        ];
        let t = meta_map(&[
            (
                10,
                flags(false, false, true, "Stanton > Crusader > Yela > A"),
            ),
            (
                20,
                flags(false, false, true, "Stanton > Crusader > Yela > B"),
            ),
        ]);
        let mut f = base_filters();
        f.scu = 50.0;
        let routes = compute(&rows, &t, &Default::default(), &f);
        assert_eq!(routes[0].commodity, "Rich"); // ProfitDesc default
        f.sort = RouteSort::ProfitAsc;
        assert_eq!(
            compute(&rows, &t, &Default::default(), &f)[0].commodity,
            "Cheap"
        );
        f.sort = RouteSort::TotalProfitDesc;
        assert_eq!(
            compute(&rows, &t, &Default::default(), &f)[0].commodity,
            "Rich"
        );
        f.sort = RouteSort::MarginDesc;
        assert_eq!(
            compute(&rows, &t, &Default::default(), &f)[0].commodity,
            "Rich"
        );
    }

    /// t_5b6d908f P3: „Profit absolut absteigend" ist redundant zu „Gewinn/SCU
    /// absteigend" — profit_total = profit_per_scu × SCU mit FESTEM SCU ist eine
    /// monotone Skalierung, die Rangfolge ist identisch. Belag: vollständige
    /// Sequenz (commodity + profit_per_scu) über mehrere Waren mit unterschied-
    /// lichen Gewinnen unter beiden Sortierungen.
    #[test]
    fn profit_total_desc_ranking_identical_to_profit_per_scu_desc() {
        use std::collections::HashMap;
        let rows = vec![
            row(11, 10, "Alpha", 100.0, 0.0),
            row(11, 20, "Alpha", 0.0, 1100.0), // gain 1000
            row(12, 10, "Beta", 200.0, 0.0),
            row(12, 20, "Beta", 0.0, 700.0), // gain 500
            row(13, 10, "Gamma", 50.0, 0.0),
            row(13, 20, "Gamma", 0.0, 930.0), // gain 880
            row(14, 10, "Delta", 80.0, 0.0),
            row(14, 20, "Delta", 0.0, 420.0), // gain 340
        ];
        let t = meta_map(&[
            (10, flags(false, false, true, "S > Crusader > Yela > A")),
            (20, flags(false, false, true, "S > Crusader > Yela > B")),
        ]);
        let order = |sort: RouteSort| -> Vec<(String, f64)> {
            let mut f = base_filters();
            f.scu = 250.0; // festes SCU für die ganze Query
            f.sort = sort;
            compute(&rows, &t, &HashMap::new(), &f)
                .into_iter()
                .map(|r| (r.commodity.clone(), r.profit_per_scu))
                .collect()
        };
        let per_scu = order(RouteSort::ProfitDesc);
        let abs_total = order(RouteSort::TotalProfitDesc);
        assert_eq!(per_scu, abs_total);
        // Erwartete Reihenfolge: Alpha(1000) → Gamma(880) → Beta(500) → Delta(340)
        let names: Vec<&str> = per_scu.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Gamma", "Beta", "Delta"]);
        // Neutral (Natural) lässt die Originalliste unverändert → NICHT Gewinn-absteigend.
        let neutral = order(RouteSort::Natural);
        assert_ne!(neutral, per_scu);
    }

    #[test]
    fn hierarchy_distance_levels() {
        assert_eq!(hierarchy_distance("A > B > C > D", "A > B > C > D"), 0);
        assert_eq!(hierarchy_distance("A > B > C > D", "A > B > C > E"), 1); // gleicher Mond
        assert_eq!(hierarchy_distance("A > B > C > D", "A > B > X > E"), 2); // gleicher Planet
        assert_eq!(hierarchy_distance("A > B > C > D", "A > Y > X > E"), 3); // anderer Planet
        assert_eq!(hierarchy_distance("A > B > C > D", "Q > Y > X > E"), 4); // anderes System
    }

    /// t_65a91d5b C3b: das optionale Enrichment-Feld ist serde-default-None —
    /// Alt-JSON (ohne Feld) und Neu-JSON (mit Feld) deserialisieren beide.
    #[test]
    fn trade_route_enrichment_serde_default_none_and_roundtrip() {
        let base = TradeRoute {
            commodity_id: 47,
            commodity: "Laranite".into(),
            illegal: false,
            buy_terminal_id: 1,
            buy_terminal: "Admin - ARC-L1".into(),
            buy_chain: "Stanton > ArcCorp > Arc-L1".into(),
            buy_price: 7400.0,
            buy_terminal_autoload: false,
            buy_terminal_cargo_deck: false,
            buy_terminal_ground: false,
            sell_terminal_id: 274,
            sell_terminal: "TDD - Hurston".into(),
            sell_chain: "Stanton > Hurston > Lorville".into(),
            sell_price: 9300.0,
            sell_terminal_autoload: false,
            sell_terminal_cargo_deck: false,
            sell_terminal_ground: true,
            sell_stock: 0.0,
            profit_per_scu: 1900.0,
            quantity: 0.0,
            profit_total: 190000.0,
            cost_total: 740000.0,
            margin: 0.2567,
            distance: 3,
            enrichment: None,
        };
        // Ohne Enrichment-Feld (Alt-JSON) → None.
        let json = serde_json::to_string(&base).unwrap();
        assert!(
            !json.contains("enrichment"),
            "None enrichment must serialize without the field: {json}"
        );
        let back: TradeRoute = serde_json::from_str(&json).unwrap();
        assert!(back.enrichment.is_none());
        // Mit Enrichment-Feld → Roundtrip.
        let mut enriched = base.clone();
        enriched.enrichment = Some(RouteEnrichment {
            uex_score: 953,
            scu_origin: 2.0,
            scu_destination: 1387.0,
            scu_reachable: 2.0,
            status_origin: 2,
            status_destination: 5,
            container_sizes_origin: "1,2,4,8".into(),
            container_sizes_destination: "1,2,4".into(),
            user_price_origin_rows: 1,
            user_price_dest_rows: 3,
            game_version_origin: Some("4.10.2".into()),
            game_version_destination: None,
            faction_origin: Some("UEE".into()),
            faction_destination: None,
            has_quantum_marker_origin: true,
            has_quantum_marker_destination: false,
            date_added: 1_793_092_627,
        });
        let json2 = serde_json::to_string(&enriched).unwrap();
        assert!(json2.contains("\"enrichment\":{\"uex_score\":953"));
        let back2: TradeRoute = serde_json::from_str(&json2).unwrap();
        let e = back2.enrichment.expect("enrichment must roundtrip");
        assert_eq!(e.uex_score, 953);
        assert_eq!(e.scu_destination, 1387.0);
        assert_eq!(e.status_destination, 5);
        assert_eq!(e.faction_destination, None);
        // Profit-Felder sind von Enrichment unberührt (K1 bleibt Owner).
        assert_eq!(back2.profit_per_scu, 1900.0);
        assert_eq!(back2.buy_price, 7400.0);
    }

    // ---- RB2 (Karte t_9b42249a): Budget-Eingabe in die Profit-Rechnung. ----

    #[test]
    fn budget_caps_quantity_and_profit() {
        // Schiff 100 SCU, Kaufpreis 10/SCU, Budget 250 → kaufbare Menge
        // = min(100, floor(250/10)=25) = 25; Profit = 25 × Gewinn/SCU.
        let rows = vec![
            row(1, 10, "Laranite", 10.0, 0.0),
            row(1, 20, "Laranite", 0.0, 70.0),
        ];
        let t = meta_map(&[
            (
                10,
                flags(false, false, true, "Stanton > Crusader > Yela > A"),
            ),
            (
                20,
                flags(false, false, true, "Stanton > Hurston > Arial > B"),
            ),
        ]);
        let mut f = base_filters();
        f.scu = 100.0;
        f.budget = Some(250.0);
        let routes = compute(&rows, &t, &Default::default(), &f);
        assert_eq!(routes.len(), 1);
        let r = &routes[0];
        assert_eq!(r.quantity, 25.0);
        assert_eq!(r.profit_per_scu, 60.0);
        assert_eq!(r.profit_total, 60.0 * 25.0);
        assert_eq!(r.cost_total, 10.0 * 25.0);
    }

    #[test]
    fn budget_above_capacity_uses_full_capacity() {
        let rows = vec![
            row(1, 10, "Laranite", 10.0, 0.0),
            row(1, 20, "Laranite", 0.0, 70.0),
        ];
        let t = meta_map(&[
            (
                10,
                flags(false, false, true, "Stanton > Crusader > Yela > A"),
            ),
            (
                20,
                flags(false, false, true, "Stanton > Hurston > Arial > B"),
            ),
        ]);
        let mut f = base_filters();
        f.scu = 100.0;
        f.budget = Some(1_000_000.0); // > 100 SCU × 10 = Kapazität
        let routes = compute(&rows, &t, &Default::default(), &f);
        let r = &routes[0];
        assert_eq!(r.quantity, 100.0);
        assert_eq!(r.profit_total, 60.0 * 100.0);
    }

    #[test]
    fn empty_budget_keeps_full_capacity_behavior() {
        let rows = vec![
            row(1, 10, "Laranite", 10.0, 0.0),
            row(1, 20, "Laranite", 0.0, 70.0),
        ];
        let t = meta_map(&[
            (
                10,
                flags(false, false, true, "Stanton > Crusader > Yela > A"),
            ),
            (
                20,
                flags(false, false, true, "Stanton > Hurston > Arial > B"),
            ),
        ]);
        // None (leer) und Some(0) → beide volle Kapazität wie heute.
        let mut f = base_filters();
        f.scu = 100.0;
        assert_eq!(
            compute(&rows, &t, &Default::default(), &f)[0].quantity,
            100.0
        );
        f.budget = Some(0.0);
        assert_eq!(
            compute(&rows, &t, &Default::default(), &f)[0].quantity,
            100.0
        );
        f.budget = None;
        assert_eq!(
            compute(&rows, &t, &Default::default(), &f)[0].quantity,
            100.0
        );
    }

    #[test]
    fn route_commodity_locations_builds_buy_and_sell_tables() {
        // Zwei Waren: Laranite wird an 2 Orten gekauft, an 2 verkauft; Titanium
        // kauft+verkauft dasselbe Terminal. Die Tabelle muss ALLE Orte liefern
        // (nicht nur die beste Buy→Sell-Kombination), mit Terminal-Name, Kette,
        // Preis und Bestands-/Nachfragewerten aus der Zeile.
        let rows = vec![
            row(1, 10, "Laranite", 100.0, 0.0), // Kaufort, buy 100
            row(1, 20, "Laranite", 90.0, 0.0),  // Kaufort, buy 90 (günstiger)
            row(1, 30, "Laranite", 0.0, 150.0), // Verkaufsort, sell 150
            row(1, 40, "Laranite", 0.0, 160.0), // Verkaufsort, sell 160 (teurer)
            row(2, 50, "Titanium", 50.0, 80.0), // kauft UND verkauft dasselbe Terminal
        ];
        let t = meta_map(&[
            (
                10,
                flags(
                    false,
                    false,
                    true,
                    "Stanton > Crusader > Daymar > Outpost A",
                ),
            ),
            (
                20,
                flags(
                    false,
                    false,
                    true,
                    "Stanton > Crusader > Daymar > Outpost B",
                ),
            ),
            (
                30,
                flags(false, false, false, "Stanton > Hurston > Lorville > TDD"),
            ),
            (
                40,
                flags(false, false, false, "Stanton > MicroTech > Port Tressler"),
            ),
            (50, flags(true, true, false, "Stanton > ArcCorp > ARC-L1")),
        ]);
        let locs = route_commodity_locations(&rows, &t, &names_of(&t), 1).unwrap();
        assert_eq!(locs.commodity, "Laranite");
        // 2 Kauforte (beide buy_price>0), 2 Verkaufsorte.
        assert_eq!(locs.buys.len(), 2);
        assert_eq!(locs.sells.len(), 2);
        // Terminal-Namen, Ketten, Preise korrekt gemappt.
        let buy_names: Vec<&str> = locs.buys.iter().map(|b| b.terminal.as_str()).collect();
        assert!(buy_names.contains(&"Terminal - Outpost A"));
        assert!(buy_names.contains(&"Terminal - Outpost B"));
        let buy20 = locs
            .buys
            .iter()
            .find(|b| b.chain.contains("Outpost B"))
            .unwrap();
        assert_eq!(buy20.price, 90.0);
        let sell40 = locs
            .sells
            .iter()
            .find(|s| s.chain.contains("Port Tressler"))
            .unwrap();
        assert_eq!(sell40.price, 160.0);
        // Flags durchgereicht: Outpost A ist Boden, Tressler ist Space.
        assert!(
            locs.buys
                .iter()
                .find(|b| b.chain.contains("Outpost A"))
                .unwrap()
                .ground
        );
        assert!(sell40.ground == false);
        assert_eq!(
            locs.sells.len(),
            2,
            "TDD + Tressler sind beide Verkaufsorte (sell_price>0)"
        );
        // Terminal 50 kauft UND verkauft Titanium → erscheint in beiden Listen.
        let ti = route_commodity_locations(&rows, &t, &names_of(&t), 2).unwrap();
        assert_eq!(ti.buys.len(), 1);
        assert_eq!(ti.sells.len(), 1);
        assert!(ti.buys[0].chain.contains("ARC-L1"));
        assert!(
            ti.sells[0].autoload,
            "Autoload-Flag des Terminals muss durchgereicht werden"
        );

        // Unbekannte Ware → None (kein leeres Rauschen).
        assert!(route_commodity_locations(&rows, &t, &names_of(&t), 99).is_none());
    }

    #[test]
    fn route_commodity_locations_rejects_non_trade_terminal_types() {
        // Regression BUG t_5a6706f7 (Stileron/Osoian Hides): die Alternativ-
        // Tabelle zeigte Ghost-Zeilen von Terminals ohne Commodity-Handel —
        // "Ship Weapons - …" (Item-Shop, buy 2530) für Osoian Hides (53) und
        // "Refinery Shop - …"/"Dumper's Depot - …" (buy 6000) für Stileron
        // (122). Diese Terminals haben KEINEN Eintrag in `terminal_flags`
        // (der Map trägt nur `is_route_terminal_type`-Terminals: commodity/
        // commodity_raw) und dürfen weder als Kauf- noch als Verkaufsort
        // auftauchen — exakt so wie compute_routes_full sie für die Matrix
        // bereits skippt. Ein legitimer Commodity-Shop bleibt in beiden
        // Richtungen sichtbar.
        let rows = vec![
            // Osoian Hides (53): Ship-Weapons-Ghost + legitimer Kaufort.
            row(53, 289, "Osoian Hides", 2530.0, 0.0), // Ship Weapons — NO flags
            row(53, 665, "Osoian Hides", 283_500.0, 0.0), // Commodity Shop — flags
            // Stileron (122): Refinery-/Dumper's-Ghosts + legitimer Verkaufsort.
            row(122, 196, "Stileron", 6000.0, 0.0), // Refinery Shop — NO flags
            row(122, 114, "Stileron", 6000.0, 0.0), // Dumper's Depot — NO flags
            row(122, 787, "Stileron", 0.0, 130_000.0), // Commodity terminal — flags
        ];
        let t = meta_map(&[
            (
                665,
                flags(false, false, false, "Pyro > Bloom > The Golden Riviera"),
            ),
            (787, flags(false, false, false, "Nyx > Delamar > Levski")),
        ]);
        let osoian = route_commodity_locations(&rows, &t, &names_of(&t), 53).unwrap();
        assert!(
            osoian.buys.iter().all(|b| b.terminal_id != 289),
            "Ship-Weapons-Terminal (kein Route-Typ) darf nicht als Kaufort stehen"
        );
        assert_eq!(osoian.buys.len(), 1, "nur der echte Commodity-Shop bleibt");
        assert_eq!(osoian.buys[0].terminal_id, 665);

        let stileron = route_commodity_locations(&rows, &t, &names_of(&t), 122).unwrap();
        assert!(
            stileron
                .buys
                .iter()
                .all(|b| b.terminal_id != 196 && b.terminal_id != 114),
            "Refinery/Dumper's Depot (kein Route-Typ) dürfen nicht als Kaufort stehen"
        );
        assert!(
            stileron.buys.is_empty(),
            "im Feed nur Verkauf — keine Kauforte"
        );
        assert_eq!(stileron.sells.len(), 1);
        assert_eq!(stileron.sells[0].terminal_id, 787);
    }

    #[test]
    fn budget_zero_quantity_route_sinks_and_is_grayed() {
        // Zwei Waren: eine mit Budget kaufbar, eine mit Budget NICHT (Menge 0).
        let rows = vec![
            // WIDOW buy 5000 → floor(4000/5000)=0 → Menge 0.
            row(1, 10, "Widow", 5000.0, 0.0),
            row(1, 20, "Widow", 0.0, 8000.0),
            // Laranite buy 100 → floor(4000/100)=40 → kaufbar.
            row(2, 30, "Laranite", 100.0, 0.0),
            row(2, 40, "Laranite", 0.0, 300.0),
        ];
        let t = meta_map(&[
            (
                10,
                flags(false, false, true, "Stanton > Crusader > Yela > A"),
            ),
            (
                20,
                flags(false, false, true, "Stanton > Hurston > Arial > B"),
            ),
            (
                30,
                flags(false, false, true, "Stanton > Crusader > Yela > C"),
            ),
            (
                40,
                flags(false, false, true, "Stanton > Hurston > Arial > D"),
            ),
        ]);
        let mut f = base_filters();
        f.scu = 100.0;
        f.budget = Some(4000.0);
        // ProfitDesc: Laranite (Gewinn/SCU 200) > Widow (Gewinn/SCU 3000),
        // aber Widow hat Menge 0 → sinkt ans Ende.
        let routes = compute(&rows, &t, &Default::default(), &f);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].commodity, "Laranite", "Menge>0 muss oben stehen");
        assert_eq!(routes[1].commodity, "Widow", "Menge 0 muss unten stehen");
        assert_eq!(routes[1].quantity, 0.0);
        assert_eq!(routes[1].profit_total, 0.0);
    }

    #[test]
    fn budget_quantity_serde_roundtrip() {
        // quantity ist Teil des Wire-Formats (Frontend will die Menge anzeigen)
        // und serde-default (Alt-JSON ohne Feld deserialisiert weiter).
        let base = TradeRoute {
            commodity_id: 1,
            commodity: "Laranite".into(),
            illegal: false,
            buy_terminal_id: 1,
            buy_terminal: "A".into(),
            buy_chain: String::new(),
            buy_price: 10.0,
            buy_terminal_autoload: false,
            buy_terminal_cargo_deck: false,
            buy_terminal_ground: false,
            sell_terminal_id: 2,
            sell_terminal: "B".into(),
            sell_chain: String::new(),
            sell_price: 70.0,
            sell_terminal_autoload: false,
            sell_terminal_cargo_deck: false,
            sell_terminal_ground: false,
            sell_stock: 0.0,
            profit_per_scu: 60.0,
            quantity: 25.0,
            profit_total: 1500.0,
            cost_total: 250.0,
            margin: 6.0,
            distance: 1,
            enrichment: None,
        };
        let back: TradeRoute =
            serde_json::from_str(&serde_json::to_string(&base).unwrap()).unwrap();
        assert_eq!(back.quantity, 25.0);
        assert_eq!(back.profit_total, 1500.0);
        // Alt-JSON ohne quantity-Feld → default 0.0 (kein Fehler).
        let mut bare = serde_json::to_value(&base).unwrap();
        bare.as_object_mut().unwrap().remove("quantity");
        let back2: TradeRoute = serde_json::from_value(bare).unwrap();
        assert_eq!(back2.quantity, 0.0);
    }
}
