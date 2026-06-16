#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPickerOption {
    pub provider: String,
    pub model: String,
    pub identity: String,
    pub display: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPickerOption {
    pub name: String,
    pub description: Option<String>,
    pub display: String,
    pub active: bool,
    pub builtin: bool,
}
