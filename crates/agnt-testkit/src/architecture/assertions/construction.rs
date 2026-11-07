//! ARCH-9.b: HTTP clients and TLS installers live only at their recorded homes.
//!
//! The construction rule is ARCH-9's first half: infrastructure is built by the
//! composition root, not by the components that use it. The census reads parsed
//! expressions — a call is a call, and a definition is not one — so an
//! installer's `ItemFn` never fires and a mention in a doc comment never fires.

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprMethodCall};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

use super::support::labels;

// region:    --- Functions

/// WHEN a call constructs an HTTP client or installs a TLS provider, THEN the
/// construction SHALL sit at its recorded home and the assertion SHALL report
/// the call's site.
///
/// A `Client::builder` / `Client::new` call matches on the type ident exactly:
/// `AuthClient::new` is a different type and never fires. The installer's own
/// definition is an `ItemFn`, not a call, so it never fires either.
pub fn arch_9b_infrastructure_construction(ws: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for label in labels(ws) {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        let mut sites = Sites::default();
        sites.visit_file(&file);
        for (line, callee) in sites.found {
            violations.push(Violation {
                assertion: ASSERTION_CONSTRUCTION,
                site: format!("{label}:{line}"),
                detail: callee,
            });
        }
    }
    violations
}

// endregion: --- Functions

// region:    --- Support

/// The assertion a construction site outside its home breaks.
const ASSERTION_CONSTRUCTION: &str = "ARCH-9.b";

/// The callee names the census counts.
const CLIENT_METHODS: [&str; 2] = ["builder", "new"];
const TLS_INSTALLER: &str = "ensure_crypto_provider";

/// The calls one file makes that construct infrastructure.
#[derive(Default)]
struct Sites {
    found: Vec<(usize, String)>,
}

impl Sites {
    /// The last segment of `path`, when it is a call to a client constructor.
    fn client_constructor(&self, expr: &Expr) -> Option<(String, String)> {
        let Expr::Path(path) = expr else {
            return None;
        };
        let segments: Vec<String> = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let last = segments.last()?;
        if last == TLS_INSTALLER {
            return Some((segments.join("::"), TLS_INSTALLER.to_string()));
        }
        let type_ident = segments.get(segments.len().checked_sub(2)?)?;
        (type_ident == "Client" && CLIENT_METHODS.contains(&last.as_str()))
            .then(|| (segments.join("::"), last.clone()))
    }
}

impl<'ast> Visit<'ast> for Sites {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Some((callee, _)) = self.client_constructor(&node.func) {
            self.found.push((node.func.span().start().line, callee));
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method = node.method.to_string();
        if CLIENT_METHODS.contains(&method.as_str())
            && let Expr::Path(receiver) = &*node.receiver
            && receiver
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "Client")
        {
            let line = node.receiver.span().start().line;
            self.found.push((line, format!("Client::{method}")));
        }
        visit::visit_expr_method_call(self, node);
    }
}

// endregion: --- Support
