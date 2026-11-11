use rig::completion::{Usage, UsageTotals};
use rig::operation::Completion;

/// The running cost of a session's model calls, in USD.
///
/// A call nothing priced adds no cost and marks the total a lower bound, so
/// the label ends in `+` until every call is priced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionCost {
    totals: UsageTotals,
}

impl SessionCost {
    /// Record one call's cost: `None` when nothing priced the call, and
    /// `complete` false when the price covers only part of it.
    pub fn record(&mut self, cost: Option<f64>, complete: bool) {
        self.totals.calls += 1;
        match cost {
            Some(total) => {
                self.totals.cost += total;
                if !complete {
                    self.totals.unpriced += 1;
                }
            }
            None => self.totals.unpriced += 1,
        }
    }

    /// The cost as `$0.123`, ending in `+` when some calls were not priced,
    /// or `None` when no call was.
    ///
    /// `UsageTotals::cost_label` yields nothing when no call was fully
    /// priced, which would hide a real summed cost behind a tokens-only line.
    /// A session whose calls were all priced but incomplete still has a lower
    /// bound worth showing, so it gets the `+` label.
    pub fn label(&self) -> Option<String> {
        let totals = &self.totals;
        if totals.calls > 0 && totals.unpriced == totals.calls && totals.cost > 0.0 {
            return Some(format!("{}+", dollars_label(totals.cost)));
        }
        totals.cost_label()
    }
}

/// Why nothing priced a usage.
///
/// A computed-cost failure used to be invisible: the session cost line simply
/// stayed tokens-only. Naming the reason makes that explainable from the logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingSkip {
    /// The wire resolves no catalog spec: it addresses no model, or the
    /// catalog lists none for its id.
    NoSpec,
    /// The catalog lists the model but no price for it.
    NoPricing,
    /// The usage lacks an input or output token counter, so no rate can be
    /// applied.
    MissingCounters,
}

impl std::fmt::Display for PricingSkip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSpec => write!(f, "the catalog lists no spec for the model"),
            Self::NoPricing => write!(f, "the catalog lists the model with no price"),
            Self::MissingCounters => {
                write!(f, "the usage reports no input or output token count")
            }
        }
    }
}

/// What `usage` costs, or why nothing priced it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PricedOutcome {
    /// The usage was priced: the total in USD, and whether every part of it
    /// had a listed rate.
    Priced { total: f64, complete: bool },
    /// Nothing priced the usage, for the named reason.
    Skipped(PricingSkip),
}

/// What `usage` costs at the prices the wire's model is listed at.
///
/// The priced spec is the wire's own: the spec it was connected to, else the
/// catalog's entry for its id. `complete` is false when a cache part has
/// tokens but no listed rate, so the total is a lower bound.
///
/// A skip names its own reason, so a tokens-only session is explainable.
pub fn computed_cost(model: &rig::DynModel<Completion>, usage: &Usage) -> PricedOutcome {
    let Some(spec) = model.spec() else {
        return PricedOutcome::Skipped(PricingSkip::NoSpec);
    };
    let Some(pricing) = spec.pricing else {
        return PricedOutcome::Skipped(PricingSkip::NoPricing);
    };
    // `Pricing::cost` needs both counters; without them it yields nothing.
    if usage.input_tokens.is_none() || usage.output_tokens.is_none() {
        return PricedOutcome::Skipped(PricingSkip::MissingCounters);
    }
    let Some(cost) = pricing.cost(usage) else {
        return PricedOutcome::Skipped(PricingSkip::MissingCounters);
    };
    PricedOutcome::Priced {
        total: cost.total,
        complete: cost.is_complete(),
    }
}

// region:    --- Support

/// A cost in USD: tenths of a cent below a dollar, cents above.
///
/// rig's own formatter is private, so this mirrors it to keep the label
/// identical to the one `UsageTotals::cost_label` produces.
fn dollars_label(cost: f64) -> String {
    if cost < 1.0 {
        format!("${cost:.3}")
    } else {
        format!("${cost:.2}")
    }
}

// endregion: --- Support
