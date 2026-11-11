pub mod cancel;
pub mod dispatch;
pub mod input;
pub mod reducer;

#[cfg(test)]
#[path = "../../test/interaction/cancel.rs"]
mod cancel_test;

#[cfg(test)]
#[path = "../../test/interaction/dispatch.rs"]
mod dispatch_test;

#[cfg(test)]
#[path = "../../test/interaction/input.rs"]
mod input_test;

#[cfg(test)]
#[path = "../../test/interaction/reducer.rs"]
mod reducer_test;

#[cfg(test)]
#[path = "../../test/interaction/event_log.rs"]
mod event_log_test;

#[cfg(test)]
#[path = "../../test/interaction/auto_resolved_card.rs"]
mod auto_resolved_card_test;

#[cfg(test)]
#[path = "../../test/interaction/session_records/support.rs"]
mod session_records_support_test;

#[cfg(test)]
#[path = "../../test/interaction/session_records/started.rs"]
mod session_records_started_test;

#[cfg(test)]
#[path = "../../test/interaction/session_records/switched.rs"]
mod session_records_switched_test;

#[cfg(test)]
#[path = "../../test/interaction/session_records/clear.rs"]
mod session_records_clear_test;
