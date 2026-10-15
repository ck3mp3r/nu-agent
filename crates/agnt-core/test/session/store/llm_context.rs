use super::*;

#[test]
fn extract_llm_context_no_markers() {
    let entries = vec![
        StoreEntry::Message(Message::user("Hello")),
        StoreEntry::Message(Message::assistant("Hi")),
        StoreEntry::Message(Message::user("How are you?")),
        StoreEntry::Message(Message::assistant("I am fine")),
    ];

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 4);
}

/// With markers present, `extract_llm_context` uses the LATEST marker: it
/// prepends its summary as a system message (unless the summary is empty) and
/// keeps only the messages after it.
#[test]
fn extract_llm_context_marker_summary() {
    // -- Case 1: single marker with a summary
    let mut entries: Vec<StoreEntry> = (0..3)
        .flat_map(|i| {
            [
                StoreEntry::Message(Message::user(format!("msg{}", i * 2))),
                StoreEntry::Message(Message::assistant(format!("reply{}", i * 2))),
            ]
        })
        .collect();
    entries.push(StoreEntry::Marker(CompactionMarker::new(
        "Summary of first 6".to_string(),
        Utc::now(),
    )));
    for (user, assistant) in [("msg6", "reply6"), ("msg8", "reply8")] {
        entries.push(StoreEntry::Message(Message::user(user.to_string())));
        entries.push(StoreEntry::Message(Message::assistant(
            assistant.to_string(),
        )));
    }

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 5, "1 summary system msg + 4 post-marker");
    match &context[0] {
        Message::System { content } => assert_eq!(content, "Summary of first 6"),
        _ => panic!("Expected system message with summary"),
    }

    // -- Case 2: multiple markers — only the latest summary is used
    let mut entries = Vec::new();
    for i in 0..2 {
        entries.push(StoreEntry::Message(Message::user(format!("old{}", i))));
        entries.push(StoreEntry::Message(Message::assistant(format!(
            "oldr{}",
            i
        ))));
    }
    entries.push(StoreEntry::Marker(CompactionMarker::new(
        "Summary1".to_string(),
        Utc::now(),
    )));
    for i in 0..2 {
        entries.push(StoreEntry::Message(Message::user(format!("mid{}", i))));
        entries.push(StoreEntry::Message(Message::assistant(format!(
            "midr{}",
            i
        ))));
    }
    entries.push(StoreEntry::Marker(CompactionMarker::new(
        "Summary2".to_string(),
        Utc::now(),
    )));
    entries.push(StoreEntry::Message(Message::user("post0".to_string())));
    entries.push(StoreEntry::Message(Message::assistant(
        "postr0".to_string(),
    )));

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 3, "summary + 2 post-marker messages");
    match &context[0] {
        Message::System { content } => assert_eq!(content, "Summary2"),
        _ => panic!("Expected system message with Summary2"),
    }

    // -- Case 3: exactly the kept messages after the marker are included
    let mut entries = Vec::new();
    for i in 0..2 {
        entries.push(StoreEntry::Message(Message::user(format!("m{}", i * 2))));
        entries.push(StoreEntry::Message(Message::assistant(format!(
            "r{}",
            i * 2
        ))));
    }
    entries.push(StoreEntry::Marker(CompactionMarker::new(
        "S".to_string(),
        Utc::now(),
    )));
    for (user, assistant) in [("m4", "r4"), ("m6", "r6")] {
        entries.push(StoreEntry::Message(Message::user(user.to_string())));
        entries.push(StoreEntry::Message(Message::assistant(
            assistant.to_string(),
        )));
    }

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 5, "summary + 4 post-marker");
    let kept_texts: Vec<String> = context[1..]
        .iter()
        .map(|m| serde_json::to_string(m).unwrap())
        .collect();
    assert!(kept_texts[0].contains("m4"));
    assert!(kept_texts[1].contains("r4"));
    assert!(kept_texts[2].contains("m6"));
    assert!(kept_texts[3].contains("r6"));

    // -- Case 4: older markers in the kept range are not re-emitted
    let mut entries = vec![
        StoreEntry::Message(Message::user("a0".to_string())),
        StoreEntry::Message(Message::assistant("ar0".to_string())),
        StoreEntry::Message(Message::user("a1".to_string())),
        StoreEntry::Message(Message::assistant("ar1".to_string())),
    ];
    entries.push(StoreEntry::Marker(CompactionMarker::new(
        "OldSummary".to_string(),
        Utc::now(),
    )));
    entries.push(StoreEntry::Message(Message::user("b0".to_string())));
    entries.push(StoreEntry::Message(Message::assistant("br0".to_string())));
    entries.push(StoreEntry::Marker(CompactionMarker::new(
        "NewSummary".to_string(),
        Utc::now(),
    )));
    for (user, assistant) in [("k0", "kr0"), ("k1", "kr1")] {
        entries.push(StoreEntry::Message(Message::user(user.to_string())));
        entries.push(StoreEntry::Message(Message::assistant(
            assistant.to_string(),
        )));
    }

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 5, "summary + 4 post-marker messages");
    match &context[0] {
        Message::System { content } => assert_eq!(content, "NewSummary"),
        _ => panic!("Expected NewSummary system message"),
    }
    for msg in context.iter().skip(1) {
        assert!(
            !matches!(msg, Message::System { .. }),
            "Should not contain older marker summaries"
        );
    }

    // -- Case 5: an empty summary (SlidingWindow) prepends no system message
    let mut entries = Vec::new();
    for i in 0..3 {
        entries.push(StoreEntry::Message(Message::user(format!("w{}", i))));
        entries.push(StoreEntry::Message(Message::assistant(format!("wr{}", i))));
    }
    entries.push(StoreEntry::Marker(CompactionMarker::new(
        String::new(),
        Utc::now(),
    )));
    for (user, assistant) in [("w6", "wr6"), ("w8", "wr8")] {
        entries.push(StoreEntry::Message(Message::user(user.to_string())));
        entries.push(StoreEntry::Message(Message::assistant(
            assistant.to_string(),
        )));
    }

    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 4, "no summary + 4 post-marker");
    assert!(
        matches!(&context[0], Message::User { .. }),
        "Expected user message, not system message when summary is empty"
    );
}
