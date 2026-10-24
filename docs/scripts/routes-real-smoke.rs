//! Temporärer Echt-Daten-Smoke (t_128b7547): echte Route-Berechnung gegen den
//! realen SQLite-Cache + echte UEX-API (gleicher Pfad wie `get_routes`).
//! Lauf: `cargo run --bin routes-real-smoke` — NICHT Teil der Tests.

use std::sync::Arc;

use provider::DataProvider;

fn main() {
    // Kein tokio "macros"-Feature im Workspace — Runtime manuell bauen.
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(run());
}

async fn run() {
    let cache_path =
        std::path::Path::new("/home/sebo/.local/share/architects-overlay/cache.sqlite3");
    let cache = Arc::new(cache::Cache::open(cache_path).expect("cache open"));
    let uex = uex_client::UexProvider::new(cache, None).expect("uex provider");

    let filters = provider::routes::RouteFilters {
        from: Some("Levski".into()),
        scu: 24.0, // Nomad (Hangar-Referenz)
        sort: provider::routes::RouteSort::ProfitDesc,
        ..Default::default()
    };
    let list = uex.trade_routes(&filters).await.expect("trade_routes");
    println!(
        "REAL SMOKE: {} Routen von/ab Levski, swept_at={}",
        list.routes.len(),
        list.swept_at
    );
    for r in list.routes.iter().take(5) {
        let scu = 24.0;
        let cost = r.buy_price * scu;
        let gross = r.sell_price * scu;
        let profit = gross - cost;
        println!(
            "{:<28} {} ({}) -> {} ({}): Kauf {:.0} / Verkauf {:.0} | Gewinn/SCU {:.0} | Total bei 24 SCU {:.0} (Kosten {:.0}, Brutto {:.0}, Differenz {:.0}) | Marge {:.1}% | Distanz {}",
            r.commodity,
            r.buy_terminal,
            r.buy_chain.split('>').next_back().unwrap_or("").trim(),
            r.sell_terminal,
            r.sell_chain.split('>').next_back().unwrap_or("").trim(),
            r.buy_price,
            r.sell_price,
            r.profit_per_scu,
            r.profit_total,
            cost,
            gross,
            profit,
            r.margin * 100.0,
            r.distance,
        );
        assert!(
            (profit - r.profit_total).abs() < 0.01,
            "PLAUSIBILITÄT FEHLER: {profit} != {}",
            r.profit_total
        );
        assert!(r.profit_per_scu > 0.0);
        assert!(r.sell_price > r.buy_price);
    }
    println!("PLAUSIBILITÄT OK: Gewinn = Einheit × Preisdifferenz × SCU auf allen Top-Routen.");
}
