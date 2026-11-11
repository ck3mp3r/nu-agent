//! A2A orchestrator loop test suite.
//!
//! Every scenario drives the runtime-owned external task vocabulary through the
//! loop. The suite is split into topical sub-files; shared imports live here
//! and each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "a2a/cancel_before_dispatch.rs"]
mod cancel_before_dispatch;
#[path = "a2a/intake_record.rs"]
mod intake_record;
#[path = "a2a/repeat_arrival.rs"]
mod repeat_arrival;
#[path = "a2a/saturation.rs"]
mod saturation;
#[path = "a2a/stacked_attribution.rs"]
mod stacked_attribution;

// endregion: --- Modules

use super::*;
