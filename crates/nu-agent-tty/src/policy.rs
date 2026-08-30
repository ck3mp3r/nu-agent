#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
    VeryVerbose,
    Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiPolicy {
    pub quiet: bool,
    pub verbosity: Verbosity,
}

impl UiPolicy {
    pub fn allows_spinner(self) -> bool {
        !self.quiet
    }
}
