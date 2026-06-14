pub(crate) const BUILTIN_PLANNER_CONTENT: &str = include_str!("builtins/planner.md");
pub(crate) const BUILTIN_MAKER_CONTENT: &str = include_str!("builtins/maker.md");

pub(crate) const BUILTIN_PLANNER_NAME: &str = "planner";
pub(crate) const BUILTIN_MAKER_NAME: &str = "maker";

pub(crate) struct BuiltinPersona {
    pub name: &'static str,
    pub content: &'static str,
}

pub(crate) const BUILTIN_PERSONAS: &[BuiltinPersona] = &[
    BuiltinPersona {
        name: BUILTIN_PLANNER_NAME,
        content: BUILTIN_PLANNER_CONTENT,
    },
    BuiltinPersona {
        name: BUILTIN_MAKER_NAME,
        content: BUILTIN_MAKER_CONTENT,
    },
];

/// Check if a name is a built-in persona
pub(crate) fn is_builtin_persona(name: &str) -> bool {
    BUILTIN_PERSONAS.iter().any(|p| p.name == name)
}

#[cfg(test)]
mod builtins_test;
