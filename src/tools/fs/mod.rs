pub mod core;
pub mod diff;

#[cfg(test)]
#[path = "core/test.rs"]
mod core_test;

#[cfg(test)]
#[path = "read/test.rs"]
mod read_test;

#[cfg(test)]
#[path = "mutation/test.rs"]
mod mutation_test;

#[cfg(test)]
#[path = "patch/test.rs"]
mod patch_test;

#[cfg(test)]
#[path = "edit/test.rs"]
mod edit_test;

#[cfg(test)]
#[path = "diff/test.rs"]
mod diff_test;
