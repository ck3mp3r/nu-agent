#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Insert,
    Normal,
    Visual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalIntent {
    Esc,
    EnterInsert,
    EnterVisual,
    YankComplete,
    InsertChordEscape,
}

#[must_use]
pub fn transition(current: Mode, intent: ModalIntent) -> Mode {
    match (current, intent) {
        (Mode::Insert, ModalIntent::Esc | ModalIntent::InsertChordEscape) => Mode::Normal,
        (Mode::Normal, ModalIntent::EnterInsert) => Mode::Insert,
        (Mode::Normal, ModalIntent::EnterVisual) => Mode::Visual,
        (Mode::Visual, ModalIntent::Esc | ModalIntent::YankComplete) => Mode::Normal,
        _ => current,
    }
}

#[must_use]
pub fn can_enter_visual_from_focus(transcript_focused: bool) -> bool {
    transcript_focused
}

#[must_use]
pub fn enter_visual_if_transcript_focused(current: Mode, transcript_focused: bool) -> Mode {
    if can_enter_visual_from_focus(transcript_focused) {
        return transition(current, ModalIntent::EnterVisual);
    }

    current
}
