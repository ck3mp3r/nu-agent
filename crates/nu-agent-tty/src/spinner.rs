#[derive(Debug, Clone)]
pub struct SpinnerState {
    enabled: bool,
    active: bool,
    suspended: bool,
    frame_index: usize,
    frames: &'static [&'static str],
}

const BRAILLE_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const ASCII_FRAMES: &[&str] = &["-", "\\", "|", "/"];

impl SpinnerState {
    pub fn new(enabled: bool) -> Self {
        let unicode = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LANG"))
            .map(|locale| locale.to_uppercase().contains("UTF-8"))
            .unwrap_or(true);
        Self {
            enabled,
            active: false,
            suspended: false,
            frame_index: 0,
            frames: if unicode {
                BRAILLE_FRAMES
            } else {
                ASCII_FRAMES
            },
        }
    }

    #[cfg(test)]
    pub fn new_with_charset(enabled: bool, unicode: bool) -> Self {
        Self {
            enabled,
            active: false,
            suspended: false,
            frame_index: 0,
            frames: if unicode {
                BRAILLE_FRAMES
            } else {
                ASCII_FRAMES
            },
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn current_frame(&self) -> &str {
        self.frames[self.frame_index]
    }

    #[cfg(test)]
    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    pub fn start(&mut self) {
        if self.enabled {
            self.active = true;
            self.suspended = false;
            self.frame_index = 0;
        }
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.suspended = false;
        self.frame_index = 0;
    }

    pub fn suspend(&mut self) {
        if self.enabled && self.active {
            self.suspended = true;
        }
    }

    pub fn resume(&mut self) {
        if self.enabled && self.active {
            self.suspended = false;
        }
    }

    pub fn tick(&mut self) {
        if self.enabled && self.active && !self.suspended {
            self.frame_index = (self.frame_index + 1) % self.frames.len();
        }
    }
}
