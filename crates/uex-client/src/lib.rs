//! # uex_client
//!
//! UEX is the *first* implementation of [`provider::DataProvider`]. It talks to
//! the UEX API (endpoints per the research doc) and caches results. Stub only —
//! the real client lands with the UEX-Client task.

use std::collections::HashSet;

use provider::{DataProvider, MarketplaceFilter, Offer, PricePoint, Result, SearchEntities, SearchHit};

/// Concrete provider for the UEX source.
pub struct UexProvider {
    /// API key, loaded from settings at runtime (never hardcoded).
    pub api_key: String,
    /// Entity kinds that UEX knows (subset of SearchEntities).
    pub supported: HashSet<SearchEntities>,
}

impl UexProvider {
    /// Construct with the configured API key.
    pub fn new(api_key: String) -> UexProvider {
        UexProvider {
            api_key,
            supported: HashSet::from([SearchEntities::Commodities, SearchEntities::Items]),
        }
    }
}

impl DataProvider for UexProvider {
    fn search(&self, _entities: SearchEntities, _query: &str) -> Result<Vec<SearchHit>> {
        // TODO(uex-client): call UEX search endpoint, map into SearchHit.
        Ok(Vec::new())
    }

    fn prices(&self, _commodity: &str) -> Result<Vec<PricePoint>> {
        // TODO(uex-client): call UEX prices endpoint, write to cache.
        Ok(Vec::new())
    }

    fn marketplace_offers(&self, _filters: MarketplaceFilter) -> Result<Vec<Offer>> {
        // TODO(uex-client): call UEX marketplace endpoint.
        Ok(Vec::new())
    }

    fn name(&self) -> String {
        String::from("uex")
    }
}