use nu_protocol::{Spanned, engine::Closure};
use std::collections::HashMap;

/// Registry for storing and managing tool closures.
///
/// Closures are registered by name with their spans and can be retrieved for execution
/// by the ToolExecutor. The registry filters non-closure values during
/// registration to ensure type safety.
pub struct ClosureRegistry {
    closures: HashMap<String, Spanned<Closure>>,
}

impl Default for ClosureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ClosureRegistry {
    /// Creates a new empty closure registry.
    pub fn new() -> Self {
        Self {
            closures: HashMap::new(),
        }
    }

    /// Registers a closure with the given name.
    ///
    /// If a closure with the same name already exists, it will be overwritten.
    ///
    /// # Arguments
    /// * `name` - The name to register the closure under
    /// * `closure` - The spanned closure to register
    pub fn register(&mut self, name: String, closure: Spanned<Closure>) {
        self.closures.insert(name, closure);
    }

    /// Retrieves a closure by name.
    ///
    /// # Arguments
    /// * `name` - The name of the closure to retrieve
    ///
    /// # Returns
    /// Some reference to the spanned closure if found, None otherwise
    pub fn get(&self, name: &str) -> Option<&Spanned<Closure>> {
        self.closures.get(name)
    }

    /// Returns an iterator over all registered closure names.
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.closures.keys()
    }
}

#[cfg(test)]
mod tests;
