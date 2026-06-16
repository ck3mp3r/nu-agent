use crate::rendering::modal::{
    ModalIntent, Mode, can_enter_visual_from_focus, enter_visual_if_transcript_focused, transition,
};

#[test]
fn insert_to_normal_via_esc_and_insert_chord_escape() {
    struct Case {
        name: &'static str,
        intent: ModalIntent,
    }

    let cases = vec![
        Case {
            name: "esc",
            intent: ModalIntent::Esc,
        },
        Case {
            name: "insert_chord_escape",
            intent: ModalIntent::InsertChordEscape,
        },
    ];

    for case in cases {
        let got = transition(Mode::Insert, case.intent);
        assert_eq!(got, Mode::Normal, "case failed: {}", case.name);
    }
}

#[test]
fn normal_to_insert_via_enter_insert_intent() {
    let got = transition(Mode::Normal, ModalIntent::EnterInsert);
    assert_eq!(got, Mode::Insert);
}

#[test]
fn normal_to_visual_via_v_requires_transcript_focus() {
    assert!(can_enter_visual_from_focus(true));
    assert!(!can_enter_visual_from_focus(false));

    let blocked = enter_visual_if_transcript_focused(Mode::Normal, false);
    assert_eq!(blocked, Mode::Normal);

    let entered = enter_visual_if_transcript_focused(Mode::Normal, true);
    assert_eq!(entered, Mode::Visual);
}

#[test]
fn visual_to_normal_via_esc_and_yank_completion() {
    struct Case {
        name: &'static str,
        intent: ModalIntent,
    }

    let cases = vec![
        Case {
            name: "esc",
            intent: ModalIntent::Esc,
        },
        Case {
            name: "yank_complete",
            intent: ModalIntent::YankComplete,
        },
    ];

    for case in cases {
        let got = transition(Mode::Visual, case.intent);
        assert_eq!(got, Mode::Normal, "case failed: {}", case.name);
    }
}

#[test]
fn invalid_transitions_are_noop() {
    struct Case {
        name: &'static str,
        mode: Mode,
        intent: ModalIntent,
    }

    let cases = vec![
        Case {
            name: "insert_enter_insert",
            mode: Mode::Insert,
            intent: ModalIntent::EnterInsert,
        },
        Case {
            name: "insert_yank_complete",
            mode: Mode::Insert,
            intent: ModalIntent::YankComplete,
        },
        Case {
            name: "normal_esc",
            mode: Mode::Normal,
            intent: ModalIntent::Esc,
        },
        Case {
            name: "normal_yank_complete",
            mode: Mode::Normal,
            intent: ModalIntent::YankComplete,
        },
        Case {
            name: "visual_enter_insert",
            mode: Mode::Visual,
            intent: ModalIntent::EnterInsert,
        },
        Case {
            name: "visual_insert_chord_escape",
            mode: Mode::Visual,
            intent: ModalIntent::InsertChordEscape,
        },
    ];

    for case in cases {
        let got = transition(case.mode, case.intent);
        assert_eq!(got, case.mode, "case failed: {}", case.name);
    }
}
