use super::Tool;

pub fn register_a2a_tools() -> Vec<Tool> {
    vec![
        Tool::AgentList,
        Tool::GetCard,
        Tool::Send,
        Tool::Get,
        Tool::Cancel,
        Tool::List,
    ]
}
