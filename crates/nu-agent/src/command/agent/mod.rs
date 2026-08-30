mod args;
pub mod command;
pub(crate) mod input;
mod mode_execute;
mod permissions;
mod persona;
pub(crate) mod picker;
mod resolve_policy;
mod run_command;
mod runtime_build;
mod setup;
pub(crate) mod tool_defs;

pub(crate) use mode_execute::{AgentMode, resolve_agent_mode, should_enter_foreground};

pub use args::{extract_and_validate_session_flags, extract_tool_timeout, extract_tools_from_call};
pub use command::*;
pub use runtime_build::resolve_config;

#[cfg(test)]
mod test;

#[cfg(test)]
mod test_helpers;

#[cfg(test)]
mod input_test;

#[cfg(test)]
mod args_test;

#[cfg(test)]
mod permissions_test;

#[cfg(test)]
mod tool_defs_test;

#[cfg(test)]
mod picker_test;

#[cfg(test)]
mod docs_contract_test;

#[cfg(test)]
mod mode_execute_test;

#[cfg(test)]
mod a2a_card_switch_test;

#[cfg(test)]
mod resolve_policy_test;
