pub mod management;
pub mod resolver;

#[cfg(test)]
#[path = "../../test/session/management.rs"]
mod management_test;

#[cfg(test)]
#[path = "../../test/session/resolver.rs"]
mod resolver_test;
