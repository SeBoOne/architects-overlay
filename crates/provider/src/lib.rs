//! # provider
//!
//! **Architecture seam of the app.** `DataProvider` is the single interface the
//! UI talks to for trade data (search, prices, marketplace). UEX is the *first*
//! implementation (`uex_client::UexProvider`); later sources (other SC trade
//! APIs) plug in behind the same trait without touching the UI layer. DTOs live
//! in shared types below, so no implementation type ever leaks through.

mod model;

pub use crate::model::{MarketplaceFilter, Offer, PricePoint, SearchEntities, SearchHit};

/// Domain error produced by a provider. The concrete variants fill in with the
/// UEX-Client task (http, timeout, rate-limit, cache-miss …).
#[derive(Debug, Clone, serde::Serialize)]
pub enum ProviderError {
    /// Generic storage for a detail string; typed variants come later.
    Other(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, _f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        Ok(())
    }
}

/// Shortcut `Result<T>` = `Result<T, ProviderError>` used across the seam.
pub type Result<T> = std::result::Result<T, ProviderError>;

/// Implementations of a trade-data source (UEX today, more later).
///
/// NOTE: synchronous for now so the scaffold compiles dependency-free; async
/// (tokio) is decided together with the UEX-Client task, which also fills the
/// method bodies.
pub trait DataProvider {
    /// Search entities (commodities / items / locations / terminals).
    fn search(&self, entities: SearchEntities, query: &str) -> Result<Vec<SearchHit>>;

    /// Historical / current price points for one commodity or item.
    fn prices(&self, commodity: &str) -> Result<Vec<PricePoint>>;

    /// Live marketplace offers, filtered.
    fn marketplace_offers(&self, filters: MarketplaceFilter) -> Result<Vec<Offer>>;

    /// Short source id, e.g. "uex".
    fn name(&self) -> String;
}