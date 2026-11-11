use super::*;

/// The runtime re-scans the persona catalog from disk: a persona created after
/// the fixture was built appears, and a deleted one disappears.
#[tokio::test]
async fn test_runtime_rescan_available_agents_tracks_the_catalog() -> Result<()> {
    // -- Setup & Fixtures: a runtime whose cwd is the persona scan root.
    let fixture = harness::make_runtime(harness::text_model("ok"))?;
    let runtime = &fixture.runtime;

    // -- Check: no persona file yet means no scanned persona of that name.
    assert!(
        !runtime
            .rescan_available_agents()
            .iter()
            .any(|agent| agent.name == "added-later")
    );

    // -- Exec: drop a persona into the cwd scan root.
    fixture.write_persona("added-later", "---\ndescription: Added later\n---\nbody\n")?;

    // -- Check: the fresh scan sees it.
    let scanned = runtime.rescan_available_agents();
    let added = scanned
        .iter()
        .find(|agent| agent.name == "added-later")
        .ok_or("the added persona must appear in the rescan")?;
    assert_eq!(added.description.as_deref(), Some("Added later"));

    // -- Exec: delete the persona file again.
    std::fs::remove_file(fixture.root().join(".agents").join("added-later.md"))?;

    // -- Check: the fresh scan no longer lists it.
    assert!(
        !runtime
            .rescan_available_agents()
            .iter()
            .any(|agent| agent.name == "added-later"),
        "a deleted persona must not appear in the rescan"
    );
    Ok(())
}
