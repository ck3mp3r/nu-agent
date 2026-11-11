//! The ARCH-6 per-crate allowance rows.
//!
//! One file per crate. The rows are a drain backlog, not compliance: the
//! baseline only ever shrinks, and a row leaves with the item it names.

mod agnt_a2a;
mod agnt_acp;
mod agnt_cli;
mod agnt_kernel;
mod agnt_plugin;
mod agnt_render;
mod agnt_runtime;
mod agnt_storage;
mod agnt_tty;
mod agnt_tui;

/// The per-crate rows, in crate order.
pub const ARCH_6_CRATES: [(&str, &[&str]); 10] = [
    ("agnt-a2a", agnt_a2a::ROWS),
    ("agnt-acp", agnt_acp::ROWS),
    ("agnt-cli", agnt_cli::ROWS),
    ("agnt-kernel", agnt_kernel::ROWS),
    ("agnt-plugin", agnt_plugin::ROWS),
    ("agnt-render", agnt_render::ROWS),
    ("agnt-runtime", agnt_runtime::ROWS),
    ("agnt-storage", agnt_storage::ROWS),
    ("agnt-tty", agnt_tty::ROWS),
    ("agnt-tui", agnt_tui::ROWS),
];

/// The per-crate impl-member rows, in crate order.
///
/// The two arrays are the two halves of one exact set: `ROWS` holds the declared
/// items, this one the impl members, and the driver compares each half per crate
/// and requires the two to name the same crates in the same order.
pub const ARCH_6_MEMBER_CRATES: [(&str, &[&str]); 10] = [
    ("agnt-a2a", agnt_a2a::MEMBER_ROWS),
    ("agnt-acp", agnt_acp::MEMBER_ROWS),
    ("agnt-cli", agnt_cli::MEMBER_ROWS),
    ("agnt-kernel", agnt_kernel::MEMBER_ROWS),
    ("agnt-plugin", agnt_plugin::MEMBER_ROWS),
    ("agnt-render", agnt_render::MEMBER_ROWS),
    ("agnt-runtime", agnt_runtime::MEMBER_ROWS),
    ("agnt-storage", agnt_storage::MEMBER_ROWS),
    ("agnt-tty", agnt_tty::MEMBER_ROWS),
    ("agnt-tui", agnt_tui::MEMBER_ROWS),
];
