use super::*;

fn make_msg(text: &str) -> rig::message::Message {
    rig::message::Message::user(text)
}

#[test]
fn initially_empty() {
    let snap = HistorySnapshot::default();
    assert_eq!(snap.arc().lock().unwrap().len(), 0);
}

#[test]
fn update_stores_history_plus_prompt() {
    let snap = HistorySnapshot::default();
    let prompt = make_msg("prompt");
    let history = [make_msg("user: hello"), make_msg("assistant: hi")];
    snap.update(&history, &prompt);
    assert_eq!(snap.arc().lock().unwrap().len(), 3);
}

#[test]
fn update_overwrites_not_appends() {
    let snap = HistorySnapshot::default();
    let prompt = make_msg("prompt");

    snap.update(&[make_msg("a")], &prompt);
    assert_eq!(snap.arc().lock().unwrap().len(), 2);

    snap.update(&[make_msg("a"), make_msg("b"), make_msg("c")], &prompt);
    assert_eq!(snap.arc().lock().unwrap().len(), 4);
}

#[test]
fn arc_shares_same_storage() {
    let snap = HistorySnapshot::default();
    let arc = snap.arc();

    let prompt = make_msg("p");
    snap.update(&[make_msg("h")], &prompt);

    // Arc obtained before update still sees the new value
    assert_eq!(arc.lock().unwrap().len(), 2);
}
