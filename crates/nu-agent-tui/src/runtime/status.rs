use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use ratatui::text::{Line, Span};

use crate::{rendering::theme::TuiTheme, state::AppState};

const BUSY_SPINNER_FRAMES: &[&str] = &["◐", "◓", "◑", "◒"];
const IDLE_INDICATOR: &str = "○";

pub(super) fn build_status_lines(state: &AppState, active_model_identity: &str) -> Vec<String> {
    let (configured, enabled, disabled, failed) = state.mcp_counts();
    let model_phase = model_activity_label(state);

    let failure_line = format_mcp_failure_line(state, 64, 48, 100);

    let model_line = match state.active_agent_identity() {
        Some(agent) => format!(
            "Model: {} ({model_phase}) | agent: {agent}",
            ellipsize(active_model_identity, 60)
        ),
        None => format!(
            "Model: {} ({model_phase})",
            ellipsize(active_model_identity, 60)
        ),
    };

    vec![
        model_line,
        format!(
            "MCP: configured={configured} enabled={enabled} disabled={disabled} failed={failed}"
        ),
        format!(
            "LLM-visible MCP tools: {}",
            state.llm_visible_mcp_tool_count()
        ),
        failure_line,
    ]
}

#[cfg(test)]
pub(super) fn compact_status_line(
    active_model_identity: &str,
    repo_branch: Option<&str>,
    now_millis: Option<u128>,
    available_width: usize,
    theme: &TuiTheme,
) -> Line<'static> {
    format_lane_1(
        active_model_identity,
        repo_branch,
        now_millis,
        available_width,
        theme,
    )
}

#[derive(Debug, Clone)]
struct GitRepoContext {
    git_dir: PathBuf,
    common_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HeadState {
    SymbolicRef(String),
    Detached(String),
}

impl HeadState {
    fn branch_label(&self) -> Option<String> {
        match self {
            HeadState::SymbolicRef(reference) => {
                if let Some(branch) = reference.strip_prefix("refs/heads/") {
                    Some(branch.to_string())
                } else if reference.is_empty() {
                    None
                } else {
                    Some(reference.clone())
                }
            }
            HeadState::Detached(short_sha) => {
                if short_sha.is_empty() {
                    None
                } else {
                    Some(short_sha.clone())
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    exists: bool,
    len: u64,
    modified_millis: u128,
}

#[derive(Debug, Clone)]
pub(super) struct RepoBranchTracker {
    caller_cwd: Option<PathBuf>,
    repo_context: Option<GitRepoContext>,
    branch: Option<String>,
    watch_targets: Vec<PathBuf>,
    watch_stamps: Vec<FileStamp>,
    last_watch_check: Instant,
    last_fallback_probe: Instant,
    watch_check_interval: Duration,
    fallback_poll_interval: Duration,
}

impl RepoBranchTracker {
    pub(super) fn from_caller_cwd(caller_cwd: Option<PathBuf>) -> Self {
        Self::from_caller_cwd_with_intervals(
            caller_cwd,
            Duration::from_millis(300),
            Duration::from_secs(2),
        )
    }

    fn from_caller_cwd_with_intervals(
        caller_cwd: Option<PathBuf>,
        watch_check_interval: Duration,
        fallback_poll_interval: Duration,
    ) -> Self {
        let now = Instant::now();
        let mut tracker = Self {
            caller_cwd,
            repo_context: None,
            branch: None,
            watch_targets: Vec::new(),
            watch_stamps: Vec::new(),
            last_watch_check: now,
            last_fallback_probe: now,
            watch_check_interval,
            fallback_poll_interval,
        };
        tracker.refresh_branch_state();
        tracker
    }

    pub(super) fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub(super) fn tick(&mut self) {
        let now = Instant::now();
        let watch_due = now.duration_since(self.last_watch_check) >= self.watch_check_interval;
        let fallback_due =
            now.duration_since(self.last_fallback_probe) >= self.fallback_poll_interval;

        if !watch_due && !fallback_due {
            return;
        }

        if watch_due {
            self.last_watch_check = now;
        }
        if fallback_due {
            self.last_fallback_probe = now;
        }

        let watch_changed = watch_due && self.watch_targets_changed();
        if watch_changed || fallback_due {
            self.refresh_branch_state();
        }
    }

    fn refresh_branch_state(&mut self) {
        if self.caller_cwd.is_none() {
            self.repo_context = None;
            self.branch = None;
            self.watch_targets.clear();
            self.watch_stamps.clear();
            return;
        }

        if self.repo_context.is_none() {
            self.repo_context = self
                .caller_cwd
                .as_deref()
                .and_then(discover_git_repo_context);
        }

        let Some(context) = self.repo_context.as_ref() else {
            self.branch = None;
            self.watch_targets.clear();
            self.watch_stamps.clear();
            return;
        };

        let head_state = read_head_state(context);
        self.branch = head_state.as_ref().and_then(HeadState::branch_label);

        let new_targets = watch_targets_for_context(context, head_state.as_ref());
        if new_targets != self.watch_targets {
            self.watch_targets = new_targets;
        }
        self.watch_stamps = file_stamps(&self.watch_targets);
    }

    fn watch_targets_changed(&self) -> bool {
        let new_stamps = file_stamps(&self.watch_targets);
        new_stamps != self.watch_stamps
    }

    #[cfg(test)]
    pub(super) fn from_caller_cwd_for_test(
        caller_cwd: Option<PathBuf>,
        watch_check_interval: Duration,
        fallback_poll_interval: Duration,
    ) -> Self {
        Self::from_caller_cwd_with_intervals(
            caller_cwd,
            watch_check_interval,
            fallback_poll_interval,
        )
    }
}

fn discover_git_repo_context(cwd: &Path) -> Option<GitRepoContext> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let git_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string());
    if git_dir.as_os_str().is_empty() {
        return None;
    }

    let common_dir = resolve_common_dir(&git_dir);
    Some(GitRepoContext {
        git_dir,
        common_dir,
    })
}

fn resolve_common_dir(git_dir: &Path) -> PathBuf {
    let commondir_path = git_dir.join("commondir");
    let Ok(raw) = fs::read_to_string(commondir_path) else {
        return git_dir.to_path_buf();
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return git_dir.to_path_buf();
    }

    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        path
    } else {
        git_dir.join(path)
    }
}

fn read_head_state(context: &GitRepoContext) -> Option<HeadState> {
    let head_raw = fs::read_to_string(context.git_dir.join("HEAD")).ok()?;
    let head = head_raw.trim();
    if head.is_empty() {
        return None;
    }

    if let Some(reference) = head.strip_prefix("ref:") {
        let reference = reference.trim();
        if reference.is_empty() {
            return None;
        }
        return Some(HeadState::SymbolicRef(reference.to_string()));
    }

    let short = head
        .chars()
        .take_while(|ch| ch.is_ascii_hexdigit())
        .take(12)
        .collect::<String>();

    if short.len() >= 7 {
        Some(HeadState::Detached(short))
    } else {
        None
    }
}

fn watch_targets_for_context(
    context: &GitRepoContext,
    head_state: Option<&HeadState>,
) -> Vec<PathBuf> {
    let mut targets = BTreeSet::new();
    targets.insert(context.git_dir.join("HEAD"));
    targets.insert(context.common_dir.join("packed-refs"));

    if let Some(HeadState::SymbolicRef(reference)) = head_state {
        targets.insert(context.git_dir.join(reference));
        targets.insert(context.common_dir.join(reference));
    }

    targets.into_iter().collect()
}

fn file_stamps(paths: &[PathBuf]) -> Vec<FileStamp> {
    paths.iter().map(|path| file_stamp(path)).collect()
}

fn file_stamp(path: &Path) -> FileStamp {
    let Ok(metadata) = fs::metadata(path) else {
        return FileStamp {
            exists: false,
            len: 0,
            modified_millis: 0,
        };
    };

    let modified_millis = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    FileStamp {
        exists: true,
        len: metadata.len(),
        modified_millis,
    }
}

#[cfg(test)]
pub(super) fn resolve_repo_branch_for_test(caller_cwd: &Path) -> Option<String> {
    let context = discover_git_repo_context(caller_cwd)?;
    let head_state = read_head_state(&context)?;
    head_state.branch_label()
}

fn emoji_for_agent(name: &str) -> &'static str {
    match name {
        "planner" => "\u{1f9ed}",       // 🧭
        "maker" => "\u{1f6e0}\u{fe0f}", // 🛠️
        _ => {
            const POOL: &[&str] = &[
                "\u{1f98a}",
                "\u{1f419}",
                "\u{1f989}",
                "\u{1f41d}",
                "\u{1f988}",
                "\u{1f40b}",
                "\u{1f98e}",
                "\u{1fab6}",
                "\u{1f335}",
                "\u{1f344}",
                "\u{1f3b2}",
                "\u{1f9f2}",
                "\u{1f52e}",
                "\u{1fa69}",
                "\u{1f9ca}",
                "\u{1fae7}",
                "\u{1fa90}",
                "\u{1f30b}",
                "\u{1f3aa}",
                "\u{1f9ff}",
                "\u{1fac0}",
                "\u{1f9ec}",
                "\u{1fab8}",
                "\u{1f9a0}",
                "\u{1f531}",
                "\u{1f9ea}",
                "\u{1fa84}",
                "\u{1f3ad}",
                "\u{1f95d}",
                "\u{1f9a9}",
            ];
            let hash = name
                .bytes()
                .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
            POOL[(hash as usize) % POOL.len()]
        }
    }
}

fn token_string_for_state(state: &AppState) -> Option<String> {
    let current = state.latest_total_tokens.unwrap_or(0);
    let s = match state.context_window_max_tokens() {
        Some(max) if max > 0 => {
            let pct = ((current as u128).saturating_mul(100) / (max as u128)).min(100) as u64;
            format!("{} ({pct}%)", compact_token_count(current))
        }
        _ => compact_token_count(current),
    };
    Some(s)
}

pub(super) fn status_left_content(
    model: &str,
    busy_millis: Option<u128>,
    state: &AppState,
    theme: &TuiTheme,
    available_width: usize,
) -> Line<'static> {
    const SEP: &str = " ┃ ";
    const SEP_WIDTH: usize = 3;

    let indicator = status_indicator(busy_millis);
    let indicator_style = if busy_millis.is_some() {
        theme.status_running
    } else {
        theme.status_done
    };

    // Prefix: indicator(1) + " "(1) = 2 chars
    let prefix_width = 2usize;
    let budget = available_width.saturating_sub(prefix_width);

    let agent_opt = state.active_agent_identity().filter(|a| !a.is_empty());
    let agent_str: Option<String> = agent_opt.map(|agent| {
        let emoji = emoji_for_agent(agent);
        format!("{emoji} {agent}")
    });

    let agent_width = agent_str.as_ref().map(|s| SEP_WIDTH + s.chars().count());

    // Model gets remaining budget after agent
    let model_budget = if let Some(aw) = agent_width {
        // only show agent if model_len + agent fits; keep at least 1 char for model
        let min_model = 1usize;
        if budget.saturating_sub(aw) >= min_model {
            budget.saturating_sub(aw)
        } else {
            budget
        }
    } else {
        budget
    };
    let model_display = tail_ellipsize(model, model_budget);
    let model_len_used = model_display.chars().count();

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(indicator.to_string(), indicator_style));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(model_display, theme.subtle_meta));

    // Agent: only if present AND fits
    if let Some(agent_display) = agent_str {
        let needed = prefix_width + model_len_used + SEP_WIDTH + agent_display.chars().count();
        if needed <= available_width {
            spans.push(Span::styled(SEP.to_string(), theme.role_separator));
            spans.push(Span::styled(agent_display, theme.role_assistant));
        }
    }

    Line::from(spans)
}

pub(super) fn status_right_content(
    repo_branch: Option<&str>,
    state: &AppState,
    theme: &TuiTheme,
) -> Option<Line<'static>> {
    const SEP: &str = " ┃ ";

    let branch_opt = repo_branch.filter(|b| !b.is_empty());
    let token_opt = token_string_for_state(state);

    if branch_opt.is_none() && token_opt.is_none() {
        return None;
    }

    let mut spans: Vec<Span<'static>> = Vec::new();

    if let Some(branch) = branch_opt {
        let branch_str = format!("\u{e725} {branch}");
        spans.push(Span::styled(branch_str, theme.focus));
    }

    if let Some(token_str) = token_opt {
        if branch_opt.is_some() {
            spans.push(Span::styled(SEP.to_string(), theme.role_separator));
        }
        spans.push(Span::styled(token_str, theme.subtle_meta));
    }

    Some(Line::from(spans))
}

#[cfg(test)]
pub(super) fn lane_2_status_line(
    state: &AppState,
    available_width: usize,
    theme: &TuiTheme,
) -> Line<'static> {
    let current = state.latest_total_tokens.unwrap_or(0);
    let token_str = match state.context_window_max_tokens() {
        Some(max) if max > 0 => {
            let pct = ((current as u128).saturating_mul(100) / (max as u128)).min(100) as u64;
            format!("{} ({pct}%)", compact_token_count(current))
        }
        _ => compact_token_count(current),
    };
    match state.active_agent_identity().filter(|a| !a.is_empty()) {
        Some(agent) => {
            let emoji = emoji_for_agent(agent);
            let left = format!("{emoji} {agent}");
            // Emoji is 2 display cells, but .len() counts bytes. Compute visual width manually:
            let left_cells = 2 + 1 + agent.len(); // emoji(2 cells) + " "(1) + name
            let right = &token_str;
            let padding = available_width.saturating_sub(left_cells + right.len());
            Line::from(vec![
                Span::styled(left, theme.role_assistant),
                Span::raw(" ".repeat(padding)),
                Span::styled(token_str, theme.subtle_meta),
            ])
        }
        None => align_right_lane_2_line(&token_str, available_width, theme),
    }
}

#[cfg(test)]
fn align_right_lane_2_line(line: &str, available_width: usize, theme: &TuiTheme) -> Line<'static> {
    let content = if line.chars().count() <= available_width {
        let pad = available_width.saturating_sub(line.chars().count());
        format!("{}{line}", " ".repeat(pad))
    } else {
        tail_ellipsize(line, available_width)
    };
    Line::from(vec![Span::styled(content, theme.subtle_meta)])
}

fn compact_token_count(value: u64) -> String {
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

#[cfg(test)]
pub(super) fn compact_status_line_with_branch_for_test(
    active_model_identity: &str,
    repo_branch: Option<&str>,
    now_millis: Option<u128>,
    available_width: usize,
) -> Line<'static> {
    compact_status_line(
        active_model_identity,
        repo_branch,
        now_millis,
        available_width,
        &TuiTheme::default(),
    )
}

#[cfg(test)]
fn format_lane_1(
    model: &str,
    repo_branch: Option<&str>,
    now_millis: Option<u128>,
    available_width: usize,
    theme: &TuiTheme,
) -> Line<'static> {
    let indicator = status_indicator(now_millis);
    let prefix_width = 2usize; // indicator(1) + " "(1)
    let inner_width = available_width.saturating_sub(prefix_width);
    let display_model = model.to_string();

    // Determine indicator style: use status_running when busy, status_done when idle.
    let indicator_style = if now_millis.is_some() {
        theme.status_running
    } else {
        theme.status_done
    };

    match repo_branch.filter(|branch| !branch.is_empty()) {
        Some(branch) => {
            let (model_segment, padding_str, branch_segment) =
                format_lane_1_parts(&display_model, branch, inner_width);
            Line::from(vec![
                Span::styled(indicator.to_string(), indicator_style),
                Span::raw(" "),
                Span::styled(model_segment, theme.subtle_meta),
                Span::raw(padding_str),
                Span::styled(branch_segment, theme.focus),
            ])
        }
        None => {
            let model_segment = tail_ellipsize(&display_model, inner_width);
            Line::from(vec![
                Span::styled(indicator.to_string(), indicator_style),
                Span::raw(" "),
                Span::styled(model_segment, theme.subtle_meta),
            ])
        }
    }
}
/// Nerd Font / Powerline git glyph prepended before the branch label
/// to denote that the displayed text is a git branch (or detached HEAD SHA).
/// Width: 2 cells (glyph + space). When the available branch budget is too
/// narrow to fit even the icon plus a single label character, the icon is
/// dropped and the raw label is ellipsized as before.
#[cfg(test)]
const BRANCH_ICON_PREFIX: &str = "\u{e725} ";
#[cfg(test)]
const BRANCH_ICON_PREFIX_WIDTH: usize = 2;#[cfg(test)]
fn format_lane_1_parts(
    model: &str,
    branch: &str,
    available_width: usize,
) -> (String, String, String) {
    let fields_max = available_width;

    if fields_max == 0 {
        return (String::new(), String::new(), String::new());
    }

    // Keep a minimum visual gap between left and right segments when possible.
    let gap_min = usize::from(fields_max > 1);
    let fields_budget = fields_max.saturating_sub(gap_min);

    let model_chars = model.chars().count();
    let branch_chars = branch.chars().count();
    let branch_display_chars = branch_chars.saturating_add(BRANCH_ICON_PREFIX_WIDTH);

    let (model_max, branch_max) = if model_chars + branch_display_chars <= fields_budget {
        (model_chars, branch_display_chars)
    } else {
        let model_only_budget = fields_budget.saturating_sub(branch_display_chars);
        if model_only_budget > 3 {
            (model_only_budget, branch_display_chars)
        } else {
            let branch_budget = fields_budget / 2;
            (fields_budget.saturating_sub(branch_budget), branch_budget)
        }
    };

    let model_segment = tail_ellipsize(model, model_max);
    let branch_segment = format_branch_segment(branch, branch_max);
    let padding = fields_budget
        .saturating_sub(model_segment.chars().count() + branch_segment.chars().count())
        + gap_min;

    (model_segment, " ".repeat(padding), branch_segment)
}

/// Ellipsize the branch label to fit `branch_max` cells while preserving the
/// trailing icon. When the budget is too small to accommodate the icon plus at
/// least one label character (i.e. < icon_width + 1), drop the icon and fall
/// back to plain `tail_ellipsize` on the raw label so layout stays stable in
/// extreme-narrow viewports.
#[cfg(test)]
fn format_branch_segment(branch: &str, branch_max: usize) -> String {
    if branch_max <= BRANCH_ICON_PREFIX_WIDTH {
        return tail_ellipsize(branch, branch_max);
    }
    let label_budget = branch_max - BRANCH_ICON_PREFIX_WIDTH;
    let label = tail_ellipsize(branch, label_budget);
    format!("{BRANCH_ICON_PREFIX}{label}")
}

fn tail_ellipsize(input: &str, max_chars: usize) -> String {
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

fn ellipsize(input: &str, max_chars: usize) -> String {
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

fn status_indicator(now_millis: Option<u128>) -> &'static str {
    match now_millis {
        Some(ms) => {
            let idx = ((ms / 150) % BUSY_SPINNER_FRAMES.len() as u128) as usize;
            BUSY_SPINNER_FRAMES[idx]
        }
        None => IDLE_INDICATOR,
    }
}

pub(super) fn model_activity_label(state: &AppState) -> &'static str {
    match state.phase {
        crate::state::UiPhase::Busy | crate::state::UiPhase::AbortPending => "busy",
        crate::state::UiPhase::Idle => {
            if state.status_line == "Thinking..." || state.status_line.starts_with("Tool: ") {
                "busy"
            } else {
                "idle"
            }
        }
    }
}

fn format_mcp_failure_line(
    state: &AppState,
    max_name_chars: usize,
    max_reason_chars: usize,
    max_line_chars: usize,
) -> String {
    let failures = state.failed_mcp_servers_with_reasons();
    if failures.is_empty() {
        return "Failures: none (healthy)".to_string();
    }

    let joined = failures
        .into_iter()
        .map(|(name, reason)| match reason {
            Some(reason) => format!(
                "{} ({})",
                ellipsize(name, max_name_chars),
                ellipsize(reason, max_reason_chars)
            ),
            None => ellipsize(name, max_name_chars),
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!("Failures: {}", ellipsize(&joined, max_line_chars))
}

pub(super) fn availability_label(availability: Option<bool>) -> &'static str {
    match availability {
        Some(true) => "available",
        Some(false) => "unavailable",
        None => "unknown",
    }
}

#[cfg(test)]
pub(super) fn status_indicator_for_test(now_millis: Option<u128>) -> &'static str {
    status_indicator(now_millis)
}
