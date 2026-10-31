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
