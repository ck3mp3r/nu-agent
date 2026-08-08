#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPickerOption {
    pub provider: String,
    pub model: String,
    pub identity: String,
    pub display: String,
    pub active: bool,
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    pub configured: bool,
    pub provider_display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelPickerRow {
    ProviderHeader { name: String, display_name: String },
    Model { option: ModelPickerOption },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPickerOption {
    pub name: String,
    pub description: Option<String>,
    pub display: String,
    pub active: bool,
    pub builtin: bool,
}
