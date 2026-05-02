use nu_protocol::{Spanned, engine::Closure};
use std::collections::HashMap;

/// Registry for storing and managing tool closures.
pub struct ClosureRegistry {
    closures: HashMap<String, Spanned<Closure>>,
}

impl Default for ClosureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ClosureRegistry {
    pub fn new() -> Self {
        Self {
            closures: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: String, closure: Spanned<Closure>) {
        self.closures.insert(name, closure);
    }

    pub fn get(&self, name: &str) -> Option<&Spanned<Closure>> {
        self.closures.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.closures.keys()
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod registry_tests;
