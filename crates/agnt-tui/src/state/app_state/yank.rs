//! The visual-selection yank: the clipboard payload the reducer asks for.

use super::AppState;

impl AppState {
    /// Builds the yank payload for the active visual-mode selection and clears
    /// it. The payload covers the selection's whole absolute visual-row range,
    /// independent of the viewport: the covering block range is rendered into
    /// an offscreen buffer at the transcript list width and scanned row by row,
    /// so the text keeps the displayed-row shape (lane prefixes, separator
    /// blanks, wrapped rows). Returns None when there is no selection or the
    /// range holds no text. The clipboard request stays with the reducer.
    pub fn yank_selection_payload(&mut self) -> Option<String> {
        let (start_row, end_row) = self.scroll.yank_selection()?;
        let payload = self.selection_text(start_row, end_row);
        if payload.is_empty() {
            None
        } else {
            Some(payload)
        }
    }
}
