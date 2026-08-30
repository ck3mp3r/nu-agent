use std::path::Path;

const BUSY_SPINNER_FRAMES: &[&str] = &["◐", "◓", "◑", "◒"];
const IDLE_INDICATOR: &str = "○";

/// Nerd Font / Powerline git glyph prepended before the branch label
/// to denote that the displayed text is a git branch (or detached HEAD SHA).
/// Width: 2 cells (glyph + space). When the available branch budget is too
/// narrow to fit even the icon plus a single label character, the icon is
/// dropped and the raw label is ellipsized as before.
pub(super) fn tail_ellipsize(input: &str, max_chars: usize) -> String {
    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }

    if max_chars == 0 {
        return String::new();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let keep = max_chars - 3;
    let suffix = input
        .chars()
        .skip(count.saturating_sub(keep))
        .collect::<String>();
    format!("...{suffix}")
}

pub(super) fn ellipsize(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }

    if max_chars == 1 {
        return "…".to_string();
    }

    let keep = max_chars - 1;
    let mut out = input.chars().take(keep).collect::<String>();
    out.push('…');
    out
}

pub(super) fn compact_token_count(value: u64) -> String {
    if value < 1_000 {
        return value.to_string();
    }

    if value < 1_000_000 {
        return compact_scaled(value, 1_000, "k");
    }

    if value < 1_000_000_000 {
        return compact_scaled(value, 1_000_000, "M");
    }

    value.to_string()
}

fn compact_scaled(value: u64, divisor: u64, suffix: &str) -> String {
    let tenths = ((value as u128).saturating_mul(10) / (divisor as u128)) as u64;
    let whole = tenths / 10;
    let frac = tenths % 10;

    if frac == 0 {
        format!("{whole}{suffix}")
    } else {
        format!("{whole}.{frac}{suffix}")
    }
}

pub(super) fn format_pwd(cwd: &Path) -> String {
    let home = std::env::var("HOME").ok();
    let path_str = cwd.to_string_lossy();
    let shortened = if let Some(home) = &home {
        if let Some(rest) = path_str.strip_prefix(home.as_str()) {
            format!("~{rest}")
        } else {
            path_str.into_owned()
        }
    } else {
        path_str.into_owned()
    };
    tail_ellipsize(&shortened, 40)
}

pub(super) fn status_indicator(now_millis: Option<u128>) -> &'static str {
    match now_millis {
        Some(ms) => {
            let idx = ((ms / 150) % BUSY_SPINNER_FRAMES.len() as u128) as usize;
            BUSY_SPINNER_FRAMES[idx]
        }
        None => IDLE_INDICATOR,
    }
}
