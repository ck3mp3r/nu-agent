use super::factory::StoreType;

/// `StoreType` parses case-insensitively and its error names the valid values.
#[test]
fn store_type_from_str() -> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Case 1: lowercase parses
    assert_eq!("memory".parse::<StoreType>()?, StoreType::Memory);

    // -- Case 2: parsing is case-insensitive
    assert_eq!("MEMORY".parse::<StoreType>()?, StoreType::Memory);
    assert_eq!("Memory".parse::<StoreType>()?, StoreType::Memory);

    // -- Case 3: the error mentions the valid values
    let err = "unknown".parse::<StoreType>().unwrap_err();
    assert!(
        err.contains("memory"),
        "error should mention 'memory': {err}"
    );
    Ok(())
}
