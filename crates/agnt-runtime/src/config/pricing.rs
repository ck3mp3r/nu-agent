//! The pricing ladder: how a servable model resolves a price.
//!
//! Outcomes, in precedence order:
//!
//! 1. the endpoint's own reported cost — rig's `priced()` and the hook's
//!    `usage.cost` match, not this module;
//! 2. the price the user configured for the model;
//! 3. the model's own row in the models cache;
//! 4. the residual: nothing prices the model, so it stays tokens-only.
//!
//! The configured price wins over the shipped row: the user stated what they
//! pay, so the catalog rate is the fallback. The catalog row reads the same
//! models cache the model picker renders, so the price a session computes and
//! the price the picker shows come from one source.
//!
//! There is no cross-vendor borrowing: a model id priced only under another
//! section stays tokens-only unless the block declares that section as its
//! `catalog`.

use rig::catalog::Pricing;

use super::models_cache::{ModelCost, ModelSpec, ModelsCache};

// region:    --- Types

/// Which rung of the pricing ladder priced a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderRung {
    /// The model's own cache row lists a cost.
    Catalog,
    /// The user configured a price for the model.
    Config,
    /// Nothing prices the model, so it stays tokens-only.
    Residual,
}

/// What the ladder resolved for one model: its prices, and the rung that
/// supplied them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedPricing {
    /// The prices in USD per million tokens, or `None` for the residual.
    pub pricing: Option<Pricing>,
    /// The rung that supplied [`Self::pricing`].
    pub rung: LadderRung,
}

impl ResolvedPricing {
    /// The outcome when no catalog row priced the model: the configured
    /// price when one is set, else the residual.
    ///
    /// This is the config and residual outcomes on their own, for a caller
    /// with no models cache to consult.
    pub fn from_config(config_pricing: Option<&ModelCost>) -> Self {
        match config_pricing {
            Some(cost) => Self {
                pricing: Some(pricing_of(cost)),
                rung: LadderRung::Config,
            },
            None => Self {
                pricing: None,
                rung: LadderRung::Residual,
            },
        }
    }
}

/// The pricing ladder over one models cache.
#[derive(Debug, Clone)]
pub struct PricingView<'a> {
    cache: &'a ModelsCache,
}

// endregion: --- Types

// region:    --- Support

/// The cache row's cost as rig prices, in the same USD-per-million units.
fn pricing_of(cost: &ModelCost) -> Pricing {
    let mut pricing = Pricing::new(cost.input, cost.output);
    if let Some(rate) = cost.cache_read {
        pricing = pricing.with_cache_read(rate);
    }
    if let Some(rate) = cost.cache_write {
        pricing = pricing.with_cache_write(rate);
    }
    pricing
}

// endregion: --- Support

// region:    --- PricingView

impl<'a> PricingView<'a> {
    /// Build the ladder over `cache`.
    pub fn new(cache: &'a ModelsCache) -> Self {
        Self { cache }
    }

    /// The ladder outcome for `provider`'s `model`.
    ///
    /// `provider` is the resolved cache section key — the block's declared
    /// `catalog`, else its name (see `PluginConfig::cache_section`).
    /// `config_pricing` is the user's price for the model; it wins over the
    /// shipped row.
    pub fn pricing_for(
        &self,
        provider: &str,
        model: &str,
        config_pricing: Option<&ModelCost>,
    ) -> ResolvedPricing {
        if let Some(cost) = config_pricing {
            return ResolvedPricing {
                pricing: Some(pricing_of(cost)),
                rung: LadderRung::Config,
            };
        }
        if let Some(cost) = self
            .cache
            .get_spec(provider, model)
            .and_then(|spec| spec.cost.as_ref())
        {
            return ResolvedPricing {
                pricing: Some(pricing_of(cost)),
                rung: LadderRung::Catalog,
            };
        }
        ResolvedPricing {
            pricing: None,
            rung: LadderRung::Residual,
        }
    }

    /// The cache row for `provider`'s `model`.
    ///
    /// `provider` is the resolved cache section key.
    pub fn cache_spec(&self, provider: &str, model: &str) -> Option<&'a ModelSpec> {
        self.cache.get_spec(provider, model)
    }
}

// endregion: --- PricingView

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/config/pricing.rs"]
mod pricing_test;

// endregion: --- Tests
