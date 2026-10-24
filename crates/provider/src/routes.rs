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
}

impl RouteSort {
    pub fn as_str(&self) -> &'static str {
        match self {
            RouteSort::ProfitDesc => "profit_desc",
            RouteSort::ProfitAsc => "profit_asc",
            RouteSort::DistanceAsc => "distance_asc",
            RouteSort::TotalProfitDesc => "total_profit_desc",
            RouteSort::MarginDesc => "margin_desc",
        }
    }
}

/// Legality-Filter: 0 = alle, 1 = nur legale, 2 = nur illegale Waren.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    pub sort: RouteSort,
}

/// Eine fertige Route: bester Einkauf → bester Verkauf einer Ware.
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
    /// Absolutgewinn = profit_per_scu × SCU-Kapazität.
    pub profit_total: f64,
    /// Einkaufskosten für die volle Kapazität (Brutto-Basis).
    pub cost_total: f64,
    /// Marge = profit_per_scu / buy_price (0..∞), gerundet auf 4 Stellen.
    pub margin: f64,
    /// Entfernung als Hierarchie-Proxy 0..=4 (siehe Modulkommentar).
    pub distance: u8,
}

/// Ergebnis der Berechnung: Routen (bereits gefiltert + sortiert) plus
/// Metadaten für die UI (Datenstand).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteList {
    pub routes: Vec<TradeRoute>,
    /// Unix-Sekunden des letzten Sweeps (Datenstand-Anzeige).
    pub swept_at: u64,
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
        if filters.cargo_deck.is_some_and(|want| meta.cargo_center != want) {
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
            profit_total: profit * scu,
            cost_total: buy.buy_price * scu,
            margin: if buy.buy_price > 0.0 {
                (profit / buy.buy_price * 1000.0).round() / 1000.0
            } else {
                0.0
            },
            distance: hierarchy_distance(&b.chain, &s.chain),
        });
    }

    sort_routes(&mut out, filters.sort);
    out
}

/// Sortiert in place nach der gewählten Reihenfolge.
fn sort_routes(routes: &mut [TradeRoute], sort: RouteSort) {
    match sort {
        RouteSort::ProfitDesc => routes.sort_by(|a, b| {
            b.profit_per_scu
                .total_cmp(&a.profit_per_scu)
                .then_with(|| a.commodity.cmp(&b.commodity))
        }),
        RouteSort::ProfitAsc => routes.sort_by(|a, b| {
            a.profit_per_scu
                .total_cmp(&b.profit_per_scu)
                .then_with(|| a.commodity.cmp(&b.commodity))
        }),
        RouteSort::DistanceAsc => routes.sort_by(|a, b| {
            a.distance
                .cmp(&b.distance)
                .then_with(|| b.profit_per_scu.total_cmp(&a.profit_per_scu))
        }),
        RouteSort::TotalProfitDesc => {
            routes.sort_by(|a, b| b.profit_total.total_cmp(&a.profit_total))
        }
        RouteSort::MarginDesc => routes.sort_by(|a, b| b.margin.total_cmp(&a.margin)),
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

    #[test]
    fn hierarchy_distance_levels() {
        assert_eq!(hierarchy_distance("A > B > C > D", "A > B > C > D"), 0);
        assert_eq!(hierarchy_distance("A > B > C > D", "A > B > C > E"), 1); // gleicher Mond
        assert_eq!(hierarchy_distance("A > B > C > D", "A > B > X > E"), 2); // gleicher Planet
        assert_eq!(hierarchy_distance("A > B > C > D", "A > Y > X > E"), 3); // anderer Planet
        assert_eq!(hierarchy_distance("A > B > C > D", "Q > Y > X > E"), 4); // anderes System
    }
}
