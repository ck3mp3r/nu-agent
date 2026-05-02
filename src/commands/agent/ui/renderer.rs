use super::event::UiEvent;

pub trait UiRenderer {
    fn emit(&mut self, event: &UiEvent);
    fn flush(&mut self);
}
