use super::*;
use crate::tools::closure::{ClosureParameter, ResolvedClosure};
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

fn create_test_resolved() -> ResolvedClosure {
    ResolvedClosure {
        closure: create_test_closure(),
        params: vec![],
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

    registry.register("add".to_string(), create_test_resolved());

    assert_eq!(registry.names().count(), 1);
    assert!(registry.get("add").is_some());
}

#[test]
fn register_multiple_closures() {
    let mut registry = ClosureRegistry::new();

    registry.register("add".to_string(), create_test_resolved());
    registry.register("multiply".to_string(), create_test_resolved());
    registry.register("divide".to_string(), create_test_resolved());

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

    registry.register("add".to_string(), create_test_resolved());
    registry.register("add".to_string(), create_test_resolved());

    assert_eq!(registry.names().count(), 1);
}

#[test]
fn names_returns_all_registered_names() {
    let mut registry = ClosureRegistry::new();

    registry.register("add".to_string(), create_test_resolved());
    registry.register("sub".to_string(), create_test_resolved());
    registry.register("mul".to_string(), create_test_resolved());

    let names: Vec<&String> = registry.names().collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&&"add".to_string()));
    assert!(names.contains(&&"sub".to_string()));
    assert!(names.contains(&&"mul".to_string()));
}

#[test]
fn register_stores_resolved_closure_with_params() {
    let mut reg = ClosureRegistry::new();
    let resolved = ResolvedClosure {
        closure: create_test_closure(),
        params: vec![ClosureParameter { name: "x".to_string(), is_required: true }],
    };
    reg.register("tool".to_string(), resolved);
    let got = reg.get("tool").unwrap();
    assert_eq!(got.params.len(), 1);
    assert_eq!(got.params[0].name, "x");
}
