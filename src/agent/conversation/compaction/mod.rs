pub(crate) mod executor;
mod guard;
mod invocation;
pub(crate) mod state;

pub(super) use guard::CompactionGuard;

#[cfg(test)]
pub(super) use invocation::execute_compaction_event_shared;

#[cfg(test)]
#[path = "test.rs"]
mod test;
