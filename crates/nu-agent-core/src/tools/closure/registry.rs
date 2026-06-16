use super::ResolvedClosure;
use std::collections::HashMap;

/// Registry for storing and managing tool closures.
pub struct ClosureRegistry {
    closures: HashMap<String, ResolvedClosure>,
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

    pub fn register(&mut self, name: String, resolved: ResolvedClosure) {
        self.closures.insert(name, resolved);
    }

    pub fn get(&self, name: &str) -> Option<&ResolvedClosure> {
        self.closures.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.closures.keys()
    }
}

#[cfg(test)]
#[path = "registry_test.rs"]
mod registry_test;
