// ---------------------------------------------------------------------------
// acceptedOutputModes validation (A2A spec §3.2.2)
// ---------------------------------------------------------------------------

/// Check a request's `acceptedOutputModes` against the modes the agent card
/// advertises in `defaultOutputModes`.
///
/// Returns `Ok(())` when the request omits the field or when at least one
/// requested mode is advertised. Returns `Err(message)` with a client-facing
/// message when the field is present and no requested mode is advertised.
pub(super) fn validate_accepted_output_modes(
    accepted: Option<&[String]>,
    advertised: &[String],
) -> Result<(), String> {
    let Some(accepted) = accepted else {
        return Ok(());
    };

    if accepted.iter().any(|mode| advertised.contains(mode)) {
        return Ok(());
    }

    Err(format!(
        "Content type not supported: acceptedOutputModes {accepted:?} does not intersect \
         the agent card defaultOutputModes {advertised:?}"
    ))
}
