use nu_plugin::EvaluatedCall;

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

fn count_short_flag_occurrences(call: &EvaluatedCall, short: char) -> usize {
    let mut count = 0;
    for (name, maybe_value) in &call.named {
        if name.item.len() == 1 && name.item.starts_with(short) {
            if let Some(nu_protocol::Value::Bool { val, .. }) = maybe_value {
                if *val {
                    count += 1;
                }
            } else {
                count += 1;
            }
        }
    }
    count
}

#[allow(clippy::result_large_err)]
fn has_true_flag(call: &EvaluatedCall, name: &str) -> Result<bool, nu_protocol::ShellError> {
    call.has_flag(name)
}

#[allow(clippy::result_large_err)]
pub fn resolve_ui_policy(call: &EvaluatedCall) -> Result<UiPolicy, nu_protocol::ShellError> {
    let quiet = has_true_flag(call, "quiet")?;
    let long_verbose = has_true_flag(call, "verbose")?;
    let short_verbose_count = count_short_flag_occurrences(call, 'v');

    let total_v = short_verbose_count + if long_verbose { 1 } else { 0 };
    let verbosity = if quiet {
        Verbosity::Quiet
    } else {
        match total_v {
            0 => Verbosity::Normal,
            1 => Verbosity::Verbose,
            2 => Verbosity::VeryVerbose,
            _ => Verbosity::Trace,
        }
    };

    Ok(UiPolicy { quiet, verbosity })
}
