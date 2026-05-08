#[path = "model_factory.rs"]
mod model_factory;

pub use model_factory::*;

#[cfg(test)]
#[path = "model/test.rs"]
mod model_tests;
