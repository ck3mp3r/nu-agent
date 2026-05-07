pub mod core;

#[cfg(test)]
#[path = "core_test.rs"]
mod core_test;

#[cfg(test)]
#[path = "read_test.rs"]
mod read_test;

#[cfg(test)]
#[path = "mutation_test.rs"]
mod mutation_test;

#[cfg(test)]
#[path = "patch_test.rs"]
mod patch_test;

#[cfg(test)]
#[path = "edit_test.rs"]
mod edit_test;
