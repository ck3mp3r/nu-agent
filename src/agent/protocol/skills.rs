use std::{
    collections::HashMap,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SkillSource {
    Local,
    Home,
}

impl SkillSource {
    pub(crate) fn priority(self) -> u8 {
        match self {
            Self::Local => 0,
            Self::Home => 1,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Home => "home",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoverableSkill {
    pub name: String,
    pub source: SkillSource,
    pub description: Option<String>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSkill {
    pub name: String,
    pub source: SkillSource,
    pub path: PathBuf,
    pub content: String,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SkillResolveError {
    InvalidSkillName(String),
    HomeSkillEscapesRoot { skill_name: String },
    Io(String),
}

impl std::fmt::Display for SkillResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSkillName(name) => {
                write!(
                    f,
                    "invalid skill name '{name}': expected a single path segment"
                )
            }
            Self::HomeSkillEscapesRoot { skill_name } => {
                write!(f, "skill '{skill_name}' resolves outside home skills root")
            }
            Self::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SkillResolveError {}

pub(crate) fn discover_skill_catalog_for_cwd(cwd: &Path) -> Vec<DiscoverableSkill> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    discover_skill_catalog_internal(cwd, home.as_deref(), None)
}

pub(crate) fn render_available_skills_preamble(cwd: &Path) -> Option<String> {
    let catalog = discover_skill_catalog_for_cwd(cwd);
    render_available_skills_preamble_from_catalog(catalog)
}

#[cfg(test)]
pub(crate) fn render_available_skills_preamble_for_tests(
    cwd: &Path,
    home: Option<&Path>,
    stop_at: Option<&Path>,
) -> Option<String> {
    let catalog = discover_skill_catalog_internal(cwd, home, stop_at);
    render_available_skills_preamble_from_catalog(catalog)
}

fn render_available_skills_preamble_from_catalog(
    catalog: Vec<DiscoverableSkill>,
) -> Option<String> {
    if catalog.is_empty() {
        return None;
    }

    let mut lines = Vec::with_capacity(catalog.len() + 3);
    lines.push(
        "Skills provide specialized instructions and workflows for specific tasks.".to_string(),
    );
    lines.push(
        "Use the skill tool to load a skill when a task matches its description. Do NOT reload a skill you have already loaded in this conversation.".to_string(),
    );
    lines.push("<available_skills>".to_string());

    for skill in catalog {
        lines.push("  <skill>".to_string());
        lines.push(format!("    <name>{}</name>", skill.name));
        if let Some(desc) = &skill.description {
            lines.push(format!("    <description>{desc}</description>"));
        }
        lines.push(format!("    <source>{}</source>", skill.source.label()));
        lines.push("  </skill>".to_string());
    }

    lines.push("</available_skills>".to_string());
    Some(lines.join("\n"))
}

#[cfg(test)]
pub(crate) fn extract_skill_description(content: &str) -> Option<String> {
    extract_skill_description_internal(content)
}

fn extract_skill_description_internal(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Skip heading lines (start with #)
        if trimmed.starts_with('#') {
            continue;
        }

        // Found first non-empty, non-heading line
        if trimmed.len() <= 150 {
            return Some(trimmed.to_string());
        }

        // Truncate to 150 chars with ellipsis
        let mut truncated = String::with_capacity(151);
        truncated.push_str(&trimmed[..150]);
        truncated.push('…');
        return Some(truncated);
    }

    None
}

pub(crate) fn resolve_explicit_skill_request_for_cwd(
    cwd: &Path,
    skill_name: &str,
) -> Result<Option<ResolvedSkill>, SkillResolveError> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    resolve_explicit_skill_request_internal(cwd, home.as_deref(), None, skill_name)
}

#[cfg(test)]
pub(crate) fn discover_skill_catalog_for_cwd_for_tests(
    cwd: &Path,
    home: Option<&Path>,
    stop_at: Option<&Path>,
) -> Vec<DiscoverableSkill> {
    discover_skill_catalog_internal(cwd, home, stop_at)
}

#[cfg(test)]
pub(crate) fn resolve_explicit_skill_request_for_cwd_for_tests(
    cwd: &Path,
    home: Option<&Path>,
    stop_at: Option<&Path>,
    skill_name: &str,
) -> Result<Option<ResolvedSkill>, SkillResolveError> {
    resolve_explicit_skill_request_internal(cwd, home, stop_at, skill_name)
}

fn discover_skill_catalog_internal(
    cwd: &Path,
    home: Option<&Path>,
    stop_at: Option<&Path>,
) -> Vec<DiscoverableSkill> {
    let mut selected: HashMap<String, ((u8, usize), DiscoverableSkill)> = HashMap::new();

    for (local_rank, root) in local_skill_roots(cwd, stop_at).into_iter().enumerate() {
        for (name, description) in discover_skill_names_in_root(&root, None) {
            let entry = DiscoverableSkill {
                name: name.clone(),
                source: SkillSource::Local,
                description,
            };
            upsert_skill_by_precedence(
                &mut selected,
                name,
                (SkillSource::Local.priority(), local_rank),
                entry,
            );
        }
    }

    if let Some(home_root) = home_skills_root(home) {
        for (name, description) in discover_skill_names_in_root(&home_root, Some(&home_root)) {
            let entry = DiscoverableSkill {
                name: name.clone(),
                source: SkillSource::Home,
                description,
            };
            upsert_skill_by_precedence(
                &mut selected,
                name,
                (SkillSource::Home.priority(), 0),
                entry,
            );
        }
    }

    let mut catalog = selected
        .into_values()
        .map(|(_, entry)| entry)
        .collect::<Vec<_>>();
    catalog.sort_by(|left, right| {
        left.source
            .priority()
            .cmp(&right.source.priority())
            .then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
    });
    catalog
}

#[cfg_attr(not(test), allow(dead_code))]
fn resolve_explicit_skill_request_internal(
    cwd: &Path,
    home: Option<&Path>,
    stop_at: Option<&Path>,
    skill_name: &str,
) -> Result<Option<ResolvedSkill>, SkillResolveError> {
    let normalized_name = normalize_skill_name(skill_name)?;

    for root in local_skill_roots(cwd, stop_at) {
        if let Some(path) = resolve_skill_path_in_root(&root, normalized_name) {
            let content = fs::read_to_string(&path).map_err(|err| {
                SkillResolveError::Io(format!("failed to read {}: {}", path.display(), err))
            })?;
            return Ok(Some(ResolvedSkill {
                name: normalized_name.to_string(),
                source: SkillSource::Local,
                path,
                content,
            }));
        }
    }

    let Some(home_root) = home_skills_root(home) else {
        return Ok(None);
    };

    let Some(path) = resolve_skill_path_in_root(&home_root, normalized_name) else {
        return Ok(None);
    };

    let canonical_home_root = fs::canonicalize(&home_root).map_err(|err| {
        SkillResolveError::Io(format!(
            "failed to canonicalize {}: {}",
            home_root.display(),
            err
        ))
    })?;
    let canonical_target = fs::canonicalize(&path).map_err(|err| {
        SkillResolveError::Io(format!(
            "failed to canonicalize {}: {}",
            path.display(),
            err
        ))
    })?;

    if !canonical_target.starts_with(&canonical_home_root) {
        return Err(SkillResolveError::HomeSkillEscapesRoot {
            skill_name: normalized_name.to_string(),
        });
    }

    let content = fs::read_to_string(&path).map_err(|err| {
        SkillResolveError::Io(format!("failed to read {}: {}", path.display(), err))
    })?;

    Ok(Some(ResolvedSkill {
        name: normalized_name.to_string(),
        source: SkillSource::Home,
        path,
        content,
    }))
}

fn upsert_skill_by_precedence(
    selected: &mut HashMap<String, ((u8, usize), DiscoverableSkill)>,
    skill_name: String,
    precedence: (u8, usize),
    entry: DiscoverableSkill,
) {
    match selected.get(&skill_name) {
        Some((existing, _)) if !is_higher_precedence(precedence, *existing) => {}
        _ => {
            selected.insert(skill_name, (precedence, entry));
        }
    }
}

fn is_higher_precedence(candidate: (u8, usize), existing: (u8, usize)) -> bool {
    candidate < existing
}

#[cfg(test)]
pub(crate) fn is_higher_precedence_for_tests(
    candidate: (u8, usize),
    existing: (u8, usize),
) -> bool {
    is_higher_precedence(candidate, existing)
}

#[cfg_attr(not(test), allow(dead_code))]
fn normalize_skill_name(skill_name: &str) -> Result<&str, SkillResolveError> {
    let trimmed = skill_name.trim();
    if trimmed.is_empty() {
        return Err(SkillResolveError::InvalidSkillName(skill_name.to_string()));
    }

    let mut components = Path::new(trimmed).components();
    let first = components.next();
    let second = components.next();
    if !matches!(first, Some(Component::Normal(_))) || second.is_some() {
        return Err(SkillResolveError::InvalidSkillName(skill_name.to_string()));
    }

    Ok(trimmed)
}

fn home_skills_root(home: Option<&Path>) -> Option<PathBuf> {
    let root = home?.join(".agents").join("skills");
    root.is_dir().then_some(root)
}

fn local_skill_roots(cwd: &Path, stop_at: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let canonical_stop = stop_at.and_then(|stop| fs::canonicalize(stop).ok());

    for ancestor in cwd.ancestors() {
        roots.push(ancestor.join(".agents").join("skills"));
        roots.push(ancestor.join("skills"));

        if let Some(stop) = canonical_stop.as_ref()
            && fs::canonicalize(ancestor)
                .map(|candidate| candidate == *stop)
                .unwrap_or(false)
        {
            break;
        }
    }

    roots
}

fn discover_skill_names_in_root(
    root: &Path,
    canonical_guard_root: Option<&Path>,
) -> Vec<(String, Option<String>)> {
    let mut entries_with_desc = Vec::new();
    let mut dedup = std::collections::HashSet::new();

    let Ok(entries) = fs::read_dir(root) else {
        return entries_with_desc;
    };

    let canonical_guard = canonical_guard_root.and_then(|path| fs::canonicalize(path).ok());

    for entry in entries.flatten() {
        let path = entry.path();

        // Shape 1: <root>/<skill-name>/SKILL.md
        if path.is_dir() {
            let Some(name) = path.file_name().and_then(|segment| segment.to_str()) else {
                continue;
            };
            let candidate = path.join("SKILL.md");
            if !candidate.is_file() {
                continue;
            }
            if !passes_canonical_guard(&candidate, canonical_guard.as_ref()) {
                continue;
            }
            if dedup.insert(name.to_string()) {
                let description = read_skill_description(&candidate);
                entries_with_desc.push((name.to_string(), description));
            }
            continue;
        }

        // Shape 2: <root>/<skill-name>.md
        if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            && path
                .file_name()
                .and_then(|file| file.to_str())
                .is_some_and(|file| file != "SKILL.md")
        {
            let Some(name) = path.file_stem().and_then(|segment| segment.to_str()) else {
                continue;
            };
            if !passes_canonical_guard(&path, canonical_guard.as_ref()) {
                continue;
            }
            if dedup.insert(name.to_string()) {
                let description = read_skill_description(&path);
                entries_with_desc.push((name.to_string(), description));
            }
        }
    }

    entries_with_desc
}

fn read_skill_description(skill_path: &Path) -> Option<String> {
    // Read first 2KB - enough for a reasonable description
    let content = fs::read(skill_path).ok()?;
    let preview = if content.len() > 2048 {
        &content[..2048]
    } else {
        &content
    };
    let text = String::from_utf8_lossy(preview);
    extract_skill_description_internal(&text)
}

fn passes_canonical_guard(path: &Path, canonical_guard_root: Option<&PathBuf>) -> bool {
    let Some(guard) = canonical_guard_root else {
        return true;
    };

    fs::canonicalize(path)
        .map(|resolved| resolved.starts_with(guard))
        .unwrap_or(false)
}

#[cfg_attr(not(test), allow(dead_code))]
fn resolve_skill_path_in_root(root: &Path, skill_name: &str) -> Option<PathBuf> {
    let directory_shape = root.join(skill_name).join("SKILL.md");
    if directory_shape.is_file() {
        return Some(directory_shape);
    }

    let flat_shape = root.join(format!("{skill_name}.md"));
    if flat_shape.is_file() {
        return Some(flat_shape);
    }

    None
}
