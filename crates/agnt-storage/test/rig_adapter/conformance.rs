//! rig-memory's own conformance suite, run against two `RigAdapter` handles
//! over one database: the whole `AsyncStore` contract in one test, kept by
//! the crate that states it.

use super::*;

/// Two handles over one empty database keep rig-memory's whole `AsyncStore`
/// contract: what a missing log or file reads as, the length check of the two
/// log writes, names refused, racing writers with exactly one winner, and a
/// conversation saved through one handle and refused through the other.
#[tokio::test]
async fn test_rig_adapter_conformance_store_contract() -> Result<()> {
    // -- Setup & Fixtures: two handles to one empty database
    let (storage, a) = memory_database("conformance")?;
    let b = RigAdapter::new(storage.clone(), "conformance");

    // -- Exec
    let check = rig_memory::conformance::check_store(&a, &b).await;

    // -- Check
    if let Err(violation) = check {
        return Err(format!("{}: {}", violation.check, violation.detail).into());
    }
    Ok(())
}
