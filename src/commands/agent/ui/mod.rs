pub mod event;
pub mod factory;
pub mod formatter;
pub mod policy;
pub mod renderer;
pub mod spinner;
pub mod stderr;

#[cfg(test)]
mod event_contract_test;

#[cfg(test)]
mod verbosity_policy_test;

#[cfg(test)]
mod tool_output_detail_test;

#[cfg(test)]
mod spinner_test;

#[cfg(test)]
mod renderer_substitution_test;

#[cfg(test)]
mod stderr_contract_test;

#[cfg(test)]
mod factory_test;
