use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct AgentsLoadResult {
    pub merged_chain: Option<String>,
    pub warnings: Vec<String>,
}

pub(crate) fn load_agents_chain_for_cwd(cwd: &Path) -> AgentsLoadResult {
    let config_dir = crate::utils::xdg::config_dir()
        .ok()
        .map(|base| base.join("nu-agent"));
    load_agents_chain_internal(cwd, config_dir.as_deref(), None)
}

#[cfg(test)]
pub(crate) fn load_agents_chain_for_cwd_for_tests(
    cwd: &Path,
    config_dir: Option<&Path>,
    stop_at: Option<&Path>,
) -> AgentsLoadResult {
    load_agents_chain_internal(cwd, config_dir, stop_at)
}

fn load_agents_chain_internal(
    cwd: &Path,
    config_dir: Option<&Path>,
    stop_at: Option<&Path>,
) -> AgentsLoadResult {
    log::debug!("load_agents_chain: cwd={cwd:?}, config_dir={config_dir:?}");
    
    let mut warnings = Vec::new();
    let mut merged_segments = Vec::new();
    let mut seen = HashSet::new();

    for candidate in discover_candidate_paths(cwd, config_dir, stop_at) {
        if !candidate.exists() {
            continue;
        }

        let canonical = canonical_path_key(&candidate);
        if !seen.insert(canonical) {
            continue;
        }

        match fs::read_to_string(&candidate) {
            Ok(content) => {
                log::debug!("load_agents_chain: loaded {:?} ({} bytes)", candidate, content.len());
                merged_segments.push(content);
            }
            Err(err) => warnings.push(format!("failed to read {}: {}", candidate.display(), err)),
        }
    }

    let merged_chain = if merged_segments.is_empty() {
        None
    } else {
        Some(merged_segments.join("\n"))
    };

    log::debug!("load_agents_chain: segments={}, merged_len={:?}", merged_segments.len(), merged_chain.as_ref().map(|c| c.len()));

    AgentsLoadResult {
        merged_chain,
        warnings,
    }
}

fn canonical_path_key(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn discover_candidate_paths(
    cwd: &Path,
    config_dir: Option<&Path>,
    stop_at: Option<&Path>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(cfg_dir) = config_dir {
        candidates.push(cfg_dir.join("agents").join("AGENTS.md"));
    }

    let mut ancestors = cwd
        .ancestors()
        .map(Path::to_path_buf)
        .collect::<Vec<PathBuf>>();
    ancestors.reverse();

    if let Some(stop) = stop_at {
        let stop_canonical = canonical_path_key(stop);
        if let Some(start_index) = ancestors
            .iter()
            .position(|p| canonical_path_key(p) == stop_canonical)
        {
            ancestors = ancestors[start_index..].to_vec();
        }
    }

    for dir in ancestors {
        candidates.push(dir.join("AGENTS.md"));
    }

    candidates.push(cwd.join("AGENTS.md"));
    candidates
}
