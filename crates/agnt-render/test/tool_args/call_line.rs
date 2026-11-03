#[test]
fn plain_call_text_keeps_text_without_an_arrow() {
    // -- Setup & Fixtures
    let line = "{ \"path\": \"a.rs\" }";

    // -- Exec & Check
    assert_eq!(super::plain_call_text(line), line);
}
