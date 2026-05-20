use super::renderer::*;

#[test]
fn render_context_stores_width() {
    let ctx = RenderContext {
        width: 80,
        cursor: false,
        selected: false,
        status: None,
        now_millis: 0,
    };
    assert_eq!(ctx.width, 80);
}

#[test]
fn render_context_stores_status_none() {
    let ctx = RenderContext {
        width: 40,
        cursor: true,
        selected: true,
        status: None,
        now_millis: 100,
    };
    assert!(ctx.status.is_none());
}

#[test]
fn render_context_stores_status_in_progress() {
    let ctx = RenderContext {
        width: 40,
        cursor: false,
        selected: false,
        status: Some(ItemStatus::InProgress),
        now_millis: 0,
    };
    assert_eq!(ctx.status, Some(ItemStatus::InProgress));
}

#[test]
fn item_status_eq() {
    assert_eq!(ItemStatus::Done, ItemStatus::Done);
    assert_ne!(ItemStatus::Done, ItemStatus::Failed);
}
