//! ARCH-9.a: the aggregate runtime exposes no publicly settable field.
//!
//! Completeness is a compile-time property of the composition root: every part
//! the aggregate needs is a constructor parameter, so omitting one does not
//! compile. A `pub` field reopens the other door — a part can be replaced after
//! the root assembled it, and a half-built aggregate becomes expressible again.

use syn::{Item, Visibility};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

use super::support::{field_line, field_name, labels, walk};

// region:    --- Functions

/// WHEN the aggregate struct declares a `pub` field, THEN the runtime SHALL be
/// publicly settable and the assertion SHALL report the field.
///
/// The checked struct is the aggregate by name: the directory holds other
/// public records (build parameters, compaction config) whose fields are data,
/// not the composition root's surface.
pub fn arch_9a_public_runtime_fields(ws: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for label in labels(ws).filter(|label| label.starts_with(AGGREGATE_SOURCES)) {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        for item in walk(&file.items) {
            let Item::Struct(item_struct) = item else {
                continue;
            };
            if !matches!(item_struct.vis, Visibility::Public(_)) {
                continue;
            }
            if item_struct.ident != AGGREGATE_TYPE {
                continue;
            }
            for (index, field) in item_struct.fields.iter().enumerate() {
                if !matches!(field.vis, Visibility::Public(_)) {
                    continue;
                }
                violations.push(Violation {
                    assertion: ASSERTION_RUNTIME_FIELDS,
                    site: format!("{label}:{}", field_line(field)),
                    detail: format!("pub {}", field_name(field, index)),
                });
            }
        }
    }
    violations
}

// endregion: --- Functions

// region:    --- Support

/// The assertion a publicly settable aggregate field breaks.
const ASSERTION_RUNTIME_FIELDS: &str = "ARCH-9.a";

/// The aggregate's sources, by the label prefix the loader gives them.
const AGGREGATE_SOURCES: &str = "agnt-runtime/src/conversation/";

/// The aggregate type's name: the struct whose fields are its surface.
const AGGREGATE_TYPE: &str = "AgentRuntime";

// endregion: --- Support
