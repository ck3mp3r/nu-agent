use super::*;
use nu_protocol::{BlockId, Span, Spanned, engine::Closure};

fn create_test_closure() -> Spanned<Closure> {
    Spanned {
        item: Closure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::unknown(),
    }
}

#[test]
fn new_registry_is_empty() {
    let registry = ClosureRegistry::new();
    assert_eq!(registry.names().count(), 0);
}

#[test]
fn register_adds_closure() {
    let mut registry = ClosureRegistry::new();
    let closure = create_test_closure();

    registry.register("add".to_string(), closure);

    assert_eq!(registry.names().count(), 1);
    assert!(registry.get("add").is_some());
}

#[test]
fn register_multiple_closures() {
    let mut registry = ClosureRegistry::new();

    registry.register("add".to_string(), create_test_closure());
    registry.register("multiply".to_string(), create_test_closure());
    registry.register("divide".to_string(), create_test_closure());

    assert_eq!(registry.names().count(), 3);
    assert!(registry.get("add").is_some());
    assert!(registry.get("multiply").is_some());
    assert!(registry.get("divide").is_some());
}

#[test]
fn get_returns_none_for_missing_closure() {
    let registry = ClosureRegistry::new();
    assert!(registry.get("nonexistent").is_none());
}

#[test]
fn register_overwrites_existing() {
    let mut registry = ClosureRegistry::new();

    registry.register("add".to_string(), create_test_closure());
    registry.register("add".to_string(), create_test_closure());

    assert_eq!(registry.names().count(), 1);
}

#[test]
fn names_returns_all_registered_names() {
    let mut registry = ClosureRegistry::new();

    registry.register("add".to_string(), create_test_closure());
    registry.register("sub".to_string(), create_test_closure());
    registry.register("mul".to_string(), create_test_closure());

    let names: Vec<&String> = registry.names().collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&&"add".to_string()));
    assert!(names.contains(&&"sub".to_string()));
    assert!(names.contains(&&"mul".to_string()));
}
