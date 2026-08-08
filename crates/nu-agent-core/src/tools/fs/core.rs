use regex::Regex;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;
use tempfile::NamedTempFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub size_bytes: u64,
    pub modified_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("version conflict: expected '{expected_version}', current '{current_version}'")]
pub struct ConflictError {
    pub expected_version: String,
    pub current_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadRequest {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadResponse {
    pub content: String,
    pub total_lines: usize,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyMutationSummary {
    pub wrote: bool,
    pub changed: bool,
    pub previous_version: String,
    pub new_version: String,
    pub previous_bytes: usize,
    pub new_bytes: usize,
    pub previous_lines: usize,
    pub new_lines: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchRange {
    pub start: usize,
    pub end: usize,
}

impl PatchRange {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn single(line: usize) -> Self {
        Self::new(line, line)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchOp {
    pub range: PatchRange,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchSummary {
    pub operation_count: usize,
    pub applied_ranges: Vec<PatchRange>,
    pub wrote: bool,
    pub changed: bool,
    pub noop: bool,
    pub conflict: bool,
    pub expected_version: String,
    pub previous_version: String,
    pub new_version: String,
    pub previous_lines: usize,
    pub new_lines: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMatchMode {
    Literal,
    Regex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOccurrence {
    First,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOperation {
    pub search: String,
    pub replacement: String,
    pub match_mode: EditMatchMode,
    pub occurrence: EditOccurrence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSummary {
    pub replacements: usize,
    pub wrote: bool,
    pub changed: bool,
    pub noop: bool,
    pub conflict: bool,
    pub expected_version: String,
    pub previous_version: String,
    pub new_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPlan {
    pub replacements: usize,
    pub would_change: bool,
    pub noop: bool,
    pub conflict: bool,
    pub expected_version: String,
    pub previous_version: String,
    pub new_version: String,
    pub previous_bytes: usize,
    pub new_bytes: usize,
    pub previous_lines: usize,
    pub new_lines: usize,
    pub previous_content: String,
    pub new_content: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MutateError {
    #[error("missing expected_version for mutating operation")]
    MissingExpectedVersion,
    #[error(transparent)]
    Conflict(#[from] ConflictError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("invalid patch range: start must be >= 1 and <= end (start={start}, end={end})")]
    InvalidPatchRangeShape { start: usize, end: usize },
    #[error("patch range out of bounds: start={start} end={end} total_lines={total_lines}")]
    PatchRangeOutOfBounds {
        start: usize,
        end: usize,
        total_lines: usize,
    },
    #[error("patch ranges overlap: [{first_start},{first_end}] with [{second_start},{second_end}]")]
    OverlappingPatchRanges {
        first_start: usize,
        first_end: usize,
        second_start: usize,
        second_end: usize,
    },
    #[error("invalid regex pattern '{pattern}': {message}")]
    InvalidRegexPattern { pattern: String, message: String },
}

pub fn version_token(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();

    digest
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>()
}

pub fn read_file(path: &Path, request: ReadRequest) -> io::Result<ReadResponse> {
    let full_content = fs::read_to_string(path)?;
    let version = version_token(&full_content);
    let lines = split_lines_preserving_terminators(&full_content);
    let total_lines = lines.len();

    let content = match (request.offset, request.limit) {
        (Some(offset), Some(limit)) => lines
            .iter()
            .skip(offset)
            .take(limit)
            .copied()
            .collect::<String>(),
        _ => full_content,
    };

    Ok(ReadResponse {
        content,
        total_lines,
        offset: request.offset,
        limit: request.limit,
        version,
    })
}

pub fn apply_full_content_mutation(
    path: &Path,
    expected_version: Option<&str>,
    new_content: &str,
) -> Result<ApplyMutationSummary, MutateError> {
    let expected = expected_version.ok_or(MutateError::MissingExpectedVersion)?;
    let current_content = fs::read_to_string(path)?;
    let current_version = version_token(&current_content);
    check_conflict(Some(expected), &current_version)?;

    let previous_lines = split_lines_preserving_terminators(&current_content).len();
    let new_lines = split_lines_preserving_terminators(new_content).len();
    let previous_bytes = current_content.len();
    let new_bytes = new_content.len();

    if current_content == new_content {
        return Ok(ApplyMutationSummary {
            wrote: false,
            changed: false,
            previous_version: current_version.clone(),
            new_version: current_version,
            previous_bytes,
            new_bytes,
            previous_lines,
            new_lines,
        });
    }

    atomic_overwrite(path, new_content.as_bytes())?;
    Ok(ApplyMutationSummary {
        wrote: true,
        changed: true,
        previous_version: current_version,
        new_version: version_token(new_content),
        previous_bytes,
        new_bytes,
        previous_lines,
        new_lines,
    })
}

pub fn apply_line_range_patch_batch(
    path: &Path,
    expected_version: Option<&str>,
    operations: Vec<PatchOp>,
) -> Result<PatchSummary, MutateError> {
    let expected = expected_version.ok_or(MutateError::MissingExpectedVersion)?;
    let current_content = fs::read_to_string(path)?;
    let current_version = version_token(&current_content);
    let previous_lines = split_lines_preserving_terminators(&current_content).len();

    if expected != current_version {
        return Ok(PatchSummary {
            operation_count: 0,
            applied_ranges: Vec::new(),
            wrote: false,
            changed: false,
            noop: false,
            conflict: true,
            expected_version: expected.to_string(),
            previous_version: current_version.clone(),
            new_version: current_version,
            previous_lines,
            new_lines: previous_lines,
        });
    }

    validate_patch_operations(&operations, previous_lines)?;

    let patched_content = apply_patch_operations_in_reverse(&current_content, &operations)?;
    let new_version = version_token(&patched_content);
    let changed = patched_content != current_content;
    let new_lines = split_lines_preserving_terminators(&patched_content).len();

    if changed {
        atomic_overwrite(path, patched_content.as_bytes())?;
    }

    Ok(PatchSummary {
        operation_count: operations.len(),
        applied_ranges: operations.iter().map(|op| op.range).collect(),
        wrote: changed,
        changed,
        noop: !changed,
        conflict: false,
        expected_version: expected.to_string(),
        previous_version: current_version,
        new_version,
        previous_lines,
        new_lines,
    })
}

pub fn apply_search_replace_edit(
    path: &Path,
    expected_version: Option<&str>,
    operation: &EditOperation,
) -> Result<EditSummary, MutateError> {
    let plan = plan_search_replace_edit(path, expected_version, operation)?;

    if plan.conflict {
        return Ok(EditSummary {
            replacements: 0,
            wrote: false,
            changed: false,
            noop: false,
            conflict: true,
            expected_version: plan.expected_version,
            previous_version: plan.previous_version,
            new_version: plan.new_version,
        });
    }

    if !plan.would_change {
        return Ok(EditSummary {
            replacements: plan.replacements,
            wrote: false,
            changed: false,
            noop: true,
            conflict: false,
            expected_version: plan.expected_version,
            previous_version: plan.previous_version,
            new_version: plan.new_version,
        });
    }

    let applied = apply_full_content_mutation(
        path,
        Some(plan.expected_version.as_str()),
        &plan.new_content,
    )?;

    Ok(EditSummary {
        replacements: plan.replacements,
        wrote: applied.wrote,
        changed: applied.changed,
        noop: !applied.changed,
        conflict: false,
        expected_version: plan.expected_version,
        previous_version: applied.previous_version,
        new_version: applied.new_version,
    })
}

pub fn plan_search_replace_edit(
    path: &Path,
    expected_version: Option<&str>,
    operation: &EditOperation,
) -> Result<EditPlan, MutateError> {
    let expected = expected_version.ok_or(MutateError::MissingExpectedVersion)?;
    let current_content = fs::read_to_string(path)?;
    let current_version = version_token(&current_content);
    let previous_lines = split_lines_preserving_terminators(&current_content).len();
    let previous_bytes = current_content.len();

    if expected != current_version {
        return Ok(EditPlan {
            replacements: 0,
            would_change: false,
            noop: false,
            conflict: true,
            expected_version: expected.to_string(),
            previous_version: current_version.clone(),
            new_version: current_version,
            previous_bytes,
            new_bytes: previous_bytes,
            previous_lines,
            new_lines: previous_lines,
            previous_content: current_content.clone(),
            new_content: current_content,
        });
    }

    let (new_content, replacements) = compute_edit_result(&current_content, operation)?;
    let new_lines = split_lines_preserving_terminators(&new_content).len();
    let new_bytes = new_content.len();
    let would_change = new_content != current_content;

    Ok(EditPlan {
        replacements,
        would_change,
        noop: !would_change,
        conflict: false,
        expected_version: expected.to_string(),
        previous_version: current_version.clone(),
        new_version: if would_change {
            version_token(&new_content)
        } else {
            current_version
        },
        previous_bytes,
        new_bytes,
        previous_lines,
        new_lines,
        previous_content: current_content,
        new_content,
    })
}

pub fn plan_create_file(path: &Path, content: &str) -> Result<EditPlan, MutateError> {
    if path.exists() {
        let current_content = fs::read_to_string(path)?;
        let current_version = version_token(&current_content);
        return Ok(EditPlan {
            replacements: 0,
            would_change: false,
            noop: false,
            conflict: true,
            expected_version: String::new(),
            previous_version: current_version.clone(),
            new_version: current_version,
            previous_bytes: current_content.len(),
            new_bytes: current_content.len(),
            previous_lines: split_lines_preserving_terminators(&current_content).len(),
            new_lines: split_lines_preserving_terminators(&current_content).len(),
            previous_content: current_content.clone(),
            new_content: current_content,
        });
    }

    let empty = "";
    let empty_version = version_token(empty);
    let new_version = version_token(content);
    let new_lines = split_lines_preserving_terminators(content).len();
    let new_bytes = content.len();

    Ok(EditPlan {
        replacements: 0,
        would_change: true,
        noop: false,
        conflict: false,
        expected_version: String::new(),
        previous_version: empty_version.clone(),
        new_version,
        previous_bytes: 0,
        new_bytes,
        previous_lines: 0,
        new_lines,
        previous_content: empty.to_string(),
        new_content: content.to_string(),
    })
}

pub fn apply_create_file(path: &Path, content: &str) -> Result<EditSummary, MutateError> {
    if path.exists() {
        let current_content = fs::read_to_string(path)?;
        let current_version = version_token(&current_content);
        return Ok(EditSummary {
            replacements: 0,
            wrote: false,
            changed: false,
            noop: false,
            conflict: true,
            expected_version: String::new(),
            previous_version: current_version.clone(),
            new_version: current_version,
        });
    }

    atomic_overwrite(path, content.as_bytes())?;

    let empty_version = version_token("");
    let new_version = version_token(content);

    Ok(EditSummary {
        replacements: 0,
        wrote: true,
        changed: true,
        noop: false,
        conflict: false,
        expected_version: String::new(),
        previous_version: empty_version,
        new_version,
    })
}

fn validate_patch_operations(
    operations: &[PatchOp],
    total_lines: usize,
) -> Result<(), MutateError> {
    for op in operations {
        if op.range.start == 0 || op.range.start > op.range.end {
            return Err(MutateError::InvalidPatchRangeShape {
                start: op.range.start,
                end: op.range.end,
            });
        }

        if op.range.end > total_lines {
            return Err(MutateError::PatchRangeOutOfBounds {
                start: op.range.start,
                end: op.range.end,
                total_lines,
            });
        }
    }

    let mut sorted_ranges = operations.iter().map(|op| op.range).collect::<Vec<_>>();
    sorted_ranges.sort_by_key(|range| (range.start, range.end));

    for pair in sorted_ranges.windows(2) {
        let first = pair[0];
        let second = pair[1];
        if second.start <= first.end {
            return Err(MutateError::OverlappingPatchRanges {
                first_start: first.start,
                first_end: first.end,
                second_start: second.start,
                second_end: second.end,
            });
        }
    }

    Ok(())
}

fn compile_regex(pattern: &str) -> Result<Regex, MutateError> {
    Regex::new(pattern).map_err(|error| MutateError::InvalidRegexPattern {
        pattern: pattern.to_string(),
        message: error.to_string(),
    })
}

fn compute_edit_result(
    content: &str,
    operation: &EditOperation,
) -> Result<(String, usize), MutateError> {
    match (operation.match_mode, operation.occurrence) {
        (EditMatchMode::Literal, EditOccurrence::First) => {
            if let Some(start) = content.find(&operation.search) {
                let end = start + operation.search.len();
                let mut edited = String::with_capacity(content.len());
                edited.push_str(&content[..start]);
                edited.push_str(&operation.replacement);
                edited.push_str(&content[end..]);
                Ok((edited, 1))
            } else {
                Ok((content.to_string(), 0))
            }
        }
        (EditMatchMode::Literal, EditOccurrence::All) => {
            let replacements = content.matches(&operation.search).count();
            if replacements == 0 {
                Ok((content.to_string(), 0))
            } else {
                Ok((
                    content.replace(&operation.search, &operation.replacement),
                    replacements,
                ))
            }
        }
        (EditMatchMode::Regex, EditOccurrence::First) => {
            let regex = compile_regex(&operation.search)?;
            if !regex.is_match(content) {
                Ok((content.to_string(), 0))
            } else {
                Ok((
                    regex
                        .replacen(content, 1, &operation.replacement)
                        .into_owned(),
                    1,
                ))
            }
        }
        (EditMatchMode::Regex, EditOccurrence::All) => {
            let regex = compile_regex(&operation.search)?;
            let replacements = regex.find_iter(content).count();
            if replacements == 0 {
                Ok((content.to_string(), 0))
            } else {
                Ok((
                    regex
                        .replace_all(content, &operation.replacement)
                        .into_owned(),
                    replacements,
                ))
            }
        }
    }
}

fn apply_patch_operations_in_reverse(
    content: &str,
    operations: &[PatchOp],
) -> Result<String, MutateError> {
    let mut lines = split_lines_preserving_terminators(content)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();

    let mut sorted_ops: Vec<&PatchOp> = operations.iter().collect();
    sorted_ops.sort_by_key(|b| std::cmp::Reverse(b.range.start));

    for op in sorted_ops {
        let start_index = op.range.start - 1;
        let end_exclusive = op.range.end;
        if end_exclusive > lines.len() {
            return Err(MutateError::PatchRangeOutOfBounds {
                start: op.range.start,
                end: op.range.end,
                total_lines: lines.len(),
            });
        }
        let replacement_lines = split_lines_preserving_terminators(&op.replacement)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        lines.splice(start_index..end_exclusive, replacement_lines);
    }

    Ok(lines.into_iter().collect())
}

fn split_lines_preserving_terminators(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }

    content.split_inclusive('\n').collect::<Vec<_>>()
}

pub fn metadata_for_path(path: &Path) -> io::Result<FileMetadata> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;
    let modified_unix_seconds = modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    Ok(FileMetadata {
        size_bytes: metadata.len(),
        modified_unix_seconds,
    })
}

pub fn check_conflict(
    expected_version: Option<&str>,
    current_version: &str,
) -> Result<(), ConflictError> {
    if let Some(expected) = expected_version
        && expected != current_version
    {
        return Err(ConflictError {
            expected_version: expected.to_string(),
            current_version: current_version.to_string(),
        });
    }

    Ok(())
}

pub fn atomic_overwrite(path: &Path, content: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot atomically overwrite path without parent directory",
        )
    })?;

    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(content)?;
    temp_file.flush()?;
    temp_file.as_file().sync_all()?;

    temp_file
        .persist(path)
        .map_err(|error| io::Error::new(error.error.kind(), error.error.to_string()))?;

    File::open(parent)?.sync_all()?;

    Ok(())
}
