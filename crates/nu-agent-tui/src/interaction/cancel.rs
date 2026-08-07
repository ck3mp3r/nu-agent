use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone)]
pub struct CancelController {
    requested: Arc<AtomicBool>,
}

impl Default for CancelController {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelController {
    pub fn new() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn request_cancel(&self) -> bool {
        !self.requested.swap(true, Ordering::SeqCst)
    }

    pub fn is_cancel_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.requested.swap(false, Ordering::SeqCst)
    }
}
