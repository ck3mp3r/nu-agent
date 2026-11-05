//! ARCH-9.b: HTTP clients and TLS installers live only at their recorded homes.

use crate::architecture::{
    Workspace, arch_9b_infrastructure_construction, assert_matches_baseline,
};

use super::support::{self, ARCH_9B, Result};

// region:    --- Fixtures

/// The crate every fixture lives in.
const CRATE: &str = "agnt-core";

/// A file the census has never recorded.
const UNRECORDED: &str = "tools/rogue.rs";

/// A client built outside the recorded homes.
const CLIENT_BUILDER: &str = "pub fn f() {\n    let _ = reqwest::Client::builder().build();\n}\n";

/// Another type named `…Client`: its `new` is not the census's term.
const AUTH_CLIENT: &str = "pub fn f() {\n    let _ = rmcp::transport::AuthClient::new();\n}\n";

/// The TLS installer called from an unrecorded file.
const TLS_INSTALL: &str =
    "pub fn f() {\n    crate::discovery::card::ensure_crypto_provider();\n}\n";

/// A client built through the method-call half of the match.
const CLIENT_METHOD: &str = "pub fn f() {\n    let _ = Client.new();\n}\n";

/// The installer's own definition: a definition is not a call.
const TLS_DEFINITION: &str = "pub(crate) fn ensure_crypto_provider() {\n    let _ = 1;\n}\n";

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the census runs against today's tree, THE violation set SHALL equal
/// the recorded baseline exactly.
#[test]
fn test_construction_arch_9b_on_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;
    let loaded = ws
        .sources()
        .iter()
        .any(|(label, _)| label == "agnt-core/src/tools/handler/http.rs");

    // -- Exec
    let found = arch_9b_infrastructure_construction(&ws);

    // -- Check
    assert!(loaded, "the recorded homes must be loaded");
    assert_matches_baseline(ARCH_9B, &found, support::baseline(ARCH_9B));
    Ok(())
}

/// WHEN `reqwest::Client::builder()` lands outside the recorded homes, THEN the
/// assertion SHALL report that file and line.
#[test]
fn test_construction_arch_9b_flags_an_unrecorded_client() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-9b-unrecorded-client")?;
    support::write_source(&root, CRATE, UNRECORDED, CLIENT_BUILDER)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_9b_infrastructure_construction(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_9B);
    assert_eq!(found[0].site, "agnt-core/src/tools/rogue.rs:2");
    assert_eq!(found[0].detail, "reqwest::Client::builder");
    Ok(())
}

/// WHEN `AuthClient::new` appears, THEN the assertion SHALL report nothing: the
/// census matches the receiver type's last ident exactly.
#[test]
fn test_construction_arch_9b_ignores_another_client_type() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-9b-auth-client")?;
    support::write_source(&root, CRATE, UNRECORDED, AUTH_CLIENT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_9b_infrastructure_construction(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a TLS installer is called from an unrecorded file, THEN the assertion
/// SHALL report the call's site.
#[test]
fn test_construction_arch_9b_flags_an_unrecorded_tls_install() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-9b-tls-install")?;
    support::write_source(&root, CRATE, UNRECORDED, TLS_INSTALL)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_9b_infrastructure_construction(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].detail,
        "crate::discovery::card::ensure_crypto_provider"
    );
    Ok(())
}

/// WHEN a client is built through a method call on `Client`, THEN the assertion
/// SHALL report that site: the method-call half matches the same rule.
#[test]
fn test_construction_arch_9b_flags_a_client_method_call() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-9b-client-method")?;
    support::write_source(&root, CRATE, UNRECORDED, CLIENT_METHOD)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_9b_infrastructure_construction(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-core/src/tools/rogue.rs:2");
    assert_eq!(found[0].detail, "Client::new");
    Ok(())
}

/// WHEN the installer's definition sits in a file, THEN the assertion SHALL
/// report nothing: a definition is not a call.
#[test]
fn test_construction_arch_9b_ignores_the_installers_definition() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-9b-tls-definition")?;
    support::write_source(&root, CRATE, UNRECORDED, TLS_DEFINITION)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_9b_infrastructure_construction(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

// endregion: --- Drivers
