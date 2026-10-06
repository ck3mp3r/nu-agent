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
#[path = "../../../test/command/agent/mode.rs"]
mod mode_test;

#[cfg(test)]
#[path = "../../../test/command/agent/signature.rs"]
mod signature_test;

#[cfg(test)]
#[path = "../../../test/command/agent/config_resolution.rs"]
mod config_resolution_test;

#[cfg(test)]
#[path = "../../../test/command/agent/new_plugin_config.rs"]
mod new_plugin_config_test;

#[cfg(test)]
#[path = "../../../test/command/agent/session.rs"]
mod session_test;

#[cfg(test)]
#[path = "../../../test/command/agent/persona_model.rs"]
mod persona_model_test;

#[cfg(test)]
#[path = "../../../test/command/agent/helpers.rs"]
mod test_helpers;

#[cfg(test)]
#[path = "../../../test/command/agent/input.rs"]
mod input_test;

#[cfg(test)]
#[path = "../../../test/command/agent/args.rs"]
mod args_test;

#[cfg(test)]
#[path = "../../../test/command/agent/permissions.rs"]
mod permissions_test;

#[cfg(test)]
#[path = "../../../test/command/agent/tool_defs.rs"]
mod tool_defs_test;

#[cfg(test)]
#[path = "../../../test/command/agent/picker.rs"]
mod picker_test;

#[cfg(test)]
#[path = "../../../test/command/agent/docs_contract.rs"]
mod docs_contract_test;

#[cfg(test)]
#[path = "../../../test/command/agent/mode_execute.rs"]
mod mode_execute_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../test/command/agent/a2a_card_switch.rs"]
mod a2a_card_switch_test;

#[cfg(test)]
#[path = "../../../test/command/agent/resolve_policy.rs"]
mod resolve_policy_test;
