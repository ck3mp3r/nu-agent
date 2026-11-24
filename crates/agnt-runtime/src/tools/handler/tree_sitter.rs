use libloading::Library;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::SystemTime;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};
use tree_sitter_language::LanguageFn;

use super::ToolHandlerError;
use crate::config::Paths;
use agnt_kernel::bus::CancelTx;

// region:    --- Types

/// A loaded grammar: the entry point into its shared library.
///
/// The library image stays mapped for the life of the process (see
/// [`load_grammar`]), so the handle needs no owner here.
struct LoadedGrammar {
    language: Language,
}

/// The grammar stores of one process: the compiled-grammar cache directory, the
/// grammars found under it, the grammar directories that hold no compiled
/// library, and the bounded parsed-tree cache.
///
/// The process assembles one of these at startup, beside [`Paths`], and threads
/// it to the AST tools, so exactly one set of stores exists and no lookup
/// reaches a global. [`GrammarStores::from_paths`] is the composition-root
/// constructor; [`GrammarStores::default`] keeps the OS default in use, the
/// fallback seam a binary that resolves no paths relies on.
pub struct GrammarStores {
    /// The injected grammar cache directory. `None` leaves the OS default in use.
    cache_dir: Option<PathBuf>,
    /// Built on the first AST call: every loaded grammar costs one `dlopen`, so
    /// a caller that never parses never pays for one.
    grammars: OnceLock<GrammarCache>,
    /// Parsed trees, recovered if a holder panicked mid-update.
    tree_cache: Mutex<BoundedTreeCache<TreeCacheEntry>>,
}

/// Language name → loaded grammar, and language name → directory of a grammar
/// that exists but holds no compiled library.
struct GrammarCache {
    loaded: HashMap<String, LoadedGrammar>,
    uncompiled: HashMap<String, PathBuf>,
}

/// Maximum number of parsed trees kept in the tree cache.
const TREE_CACHE_CAPACITY: usize = 64;

/// File path → (mtime, source text, parsed tree).
type TreeCacheEntry = (SystemTime, Arc<String>, Tree);

/// A bounded LRU cache keyed by file path. When the cache exceeds
/// `TREE_CACHE_CAPACITY` entries, the least recently used entry is evicted.
struct BoundedTreeCache<V> {
    map: HashMap<PathBuf, V>,
    order: VecDeque<PathBuf>,
}

impl<V> BoundedTreeCache<V> {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, path: &Path) -> Option<&V> {
        if self.map.contains_key(path) {
            self.touch(path);
        }
        self.map.get(path)
    }

    fn insert(&mut self, path: PathBuf, value: V) {
        if self.map.contains_key(&path) {
            self.map.insert(path.clone(), value);
            self.touch(&path);
        } else {
            if self.map.len() >= TREE_CACHE_CAPACITY
                && let Some(oldest) = self.order.pop_front()
            {
                self.map.remove(&oldest);
            }
            self.map.insert(path.clone(), value);
            self.order.push_back(path);
        }
    }

    fn touch(&mut self, path: &Path) {
        if let Some(pos) = self.order.iter().position(|p| p == path)
            && let Some(p) = self.order.remove(pos)
        {
            self.order.push_back(p);
        }
    }
}

// endregion: --- Types

// region:    --- GrammarStores

impl GrammarStores {
    /// Build the stores of one process from the resolved paths.
    ///
    /// The grammar cache keeps its own base
    /// ([`Paths::grammar_cache_dir`]), so this constructor takes the resolved
    /// directory instead of deriving it: the cache ladder has a `[paths]` level
    /// the process environment cannot see.
    ///
    /// Grammars load on the first AST call, which is always after the config
    /// load, so construction stays cheap for a process that never parses.
    pub fn from_paths(paths: &Paths) -> Self {
        log::debug!(
            "grammar cache dir resolved at {}",
            paths.grammar_cache_dir.display()
        );
        Self {
            cache_dir: Some(paths.grammar_cache_dir.clone()),
            grammars: OnceLock::new(),
            tree_cache: Mutex::new(BoundedTreeCache::new()),
        }
    }

    /// The loaded grammars, loading every compiled grammar on first use.
    fn grammars(&self) -> Result<&GrammarCache, ToolHandlerError> {
        if let Some(cache) = self.grammars.get() {
            return Ok(cache);
        }
        let cache = GrammarCache::load(self.cache_dir.as_deref())?;
        let _ = self.grammars.set(cache);
        self.grammars
            .get()
            .ok_or_else(|| ToolHandlerError::runtime("grammar cache initialization failed"))
    }

    /// The parsed-tree cache, recovered if a holder panicked mid-update.
    fn tree_cache(&self) -> MutexGuard<'_, BoundedTreeCache<TreeCacheEntry>> {
        self.tree_cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Look up a loaded grammar, building the cache on first use.
    fn get_grammar(&self, path: &Path, language: &str) -> Result<&LoadedGrammar, ToolHandlerError> {
        let cache = self.grammars()?;
        if let Some(grammar) = cache.loaded.get(language) {
            return Ok(grammar);
        }
        if let Some(dir) = cache.uncompiled.get(language) {
            return Err(ToolHandlerError::runtime(format!(
                "Grammar for {language} found at {} but not compiled. Run `tree-sitter build` in the grammar directory.",
                dir.display()
            )));
        }
        Err(ToolHandlerError::validation(format!(
            "No tree-sitter grammar found for language '{language}'. File: '{}'.\nInstall the {language} grammar: clone the tree-sitter-{language} repo and run `tree-sitter build` in it.",
            path.display(),
        )))
    }

    /// Read and parse a file, consulting the tree cache first.
    fn get_or_parse(
        &self,
        path: &Path,
        language: &str,
    ) -> Result<(Arc<String>, Tree), ToolHandlerError> {
        let metadata = std::fs::metadata(path).map_err(|_| {
            ToolHandlerError::runtime(format!("File not found: {}", path.display()))
        })?;
        let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        {
            let mut cache = self.tree_cache();
            if let Some((cached_mtime, source, tree)) = cache.get(path)
                && *cached_mtime == mtime
            {
                return Ok((source.clone(), tree.clone()));
            }
        }

        let source = std::fs::read_to_string(path).map_err(|_| {
            ToolHandlerError::runtime(format!("File not found: {}", path.display()))
        })?;
        let grammar = self.get_grammar(path, language)?;
        let mut parser = Parser::new();
        parser.set_language(&grammar.language).map_err(|e| {
            ToolHandlerError::runtime(format!("Failed to set language for {language}: {e}"))
        })?;
        let tree = parser.parse(&source, None).ok_or_else(|| {
            ToolHandlerError::runtime(format!("Failed to parse {}", path.display()))
        })?;
        let source = Arc::new(source);
        let mut cache = self.tree_cache();
        cache.insert(path.to_path_buf(), (mtime, source.clone(), tree.clone()));
        Ok((source, tree))
    }
}

impl Default for GrammarStores {
    fn default() -> Self {
        Self {
            cache_dir: None,
            grammars: OnceLock::new(),
            tree_cache: Mutex::new(BoundedTreeCache::new()),
        }
    }
}

// endregion: --- GrammarStores

/// Shared-library file extensions for the current platform.
fn shared_library_extensions() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["dll"]
    } else if cfg!(target_os = "macos") {
        &["dylib", "so"]
    } else {
        &["so"]
    }
}

/// The OS cache directory where `tree-sitter` stores compiled grammars.
///
/// This is the fallback seam: the process-environment read serves a binary that
/// installs no resolved paths, and a test that injects a directory never
/// reaches it.
fn os_cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".cache"))
        })
        .map(|p| p.join("tree-sitter"))
}

/// Locate a pre-compiled shared library for a grammar, checking the grammar's
/// `build/` directory, the grammar root, and the grammar cache `lib/`
/// subdirectory.
///
/// `cache_dir` is the injected grammar cache
/// ([`Paths::grammar_cache_dir`](crate::config::Paths::grammar_cache_dir)); the
/// `None` case falls back to the OS default.
fn find_compiled_library(
    grammar_dir: &Path,
    language: &str,
    cache_dir: Option<&Path>,
) -> Option<PathBuf> {
    let search_dirs = [grammar_dir.join("build"), grammar_dir.to_path_buf()];
    for dir in &search_dirs {
        for ext in shared_library_extensions() {
            let candidate = dir.join(format!("{language}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    if let Some(cache_dir) = cache_dir {
        let lib_dir = cache_dir.join("lib");
        for ext in shared_library_extensions() {
            let candidate = lib_dir.join(format!("{language}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Load a grammar from a pre-compiled shared library.
///
/// The image stays mapped for the rest of the process and is never unloaded:
/// the `Language` this returns points into it, and every tree and query the AST
/// tools build calls back into the same image. Unloading the grammar behind a
/// live tree crashes the process, so the handle is released without a `dlclose`
/// and the OS reclaims the image at exit — the lifetime the process-wide cache
/// this replaces always had.
fn load_grammar(language: &str, lib_path: &Path) -> Result<LoadedGrammar, ToolHandlerError> {
    let library = unsafe {
        Library::new(lib_path).map_err(|e| {
            ToolHandlerError::runtime(format!("Failed to load grammar for {language}: {e}"))
        })?
    };
    let symbol_name = format!("tree_sitter_{language}\0");
    let lang_fn = unsafe {
        let symbol: libloading::Symbol<unsafe extern "C" fn() -> *const ()> =
            library.get(symbol_name.as_bytes()).map_err(|e| {
                ToolHandlerError::runtime(format!(
                    "Failed to load grammar for {language}: symbol 'tree_sitter_{language}' not found: {e}"
                ))
            })?;
        LanguageFn::from_raw(*symbol)
    };
    std::mem::forget(library);
    Ok(LoadedGrammar {
        language: Language::new(lang_fn),
    })
}

/// Load the tree-sitter config and return its `parser-directories`.
fn load_parser_directories() -> Result<Vec<PathBuf>, ToolHandlerError> {
    let config_path = tree_sitter_config::Config::find_config_file().map_err(|e| {
        ToolHandlerError::runtime(format!("Failed to locate tree-sitter config: {e}"))
    })?;
    let Some(config_path) = config_path else {
        return Err(ToolHandlerError::runtime(
            "No tree-sitter config found. Run `tree-sitter init-config` to create one.",
        ));
    };
    let config = tree_sitter_config::Config::load(Some(config_path)).map_err(|e| {
        ToolHandlerError::runtime(format!("Failed to load tree-sitter config: {e}"))
    })?;
    #[derive(serde::Deserialize)]
    struct ParserConfig {
        #[serde(default, rename = "parser-directories")]
        parser_directories: Vec<PathBuf>,
    }
    let parser_config: ParserConfig = config.get().map_err(|e| {
        ToolHandlerError::runtime(format!("Failed to parse tree-sitter config: {e}"))
    })?;
    Ok(parser_config.parser_directories)
}

/// Scan parser directories for `tree-sitter-*` grammar repos and load their
/// pre-compiled shared libraries.
impl GrammarCache {
    fn load(cache_dir: Option<&Path>) -> Result<Self, ToolHandlerError> {
        let parser_dirs = load_parser_directories()?;
        let cache_dir = cache_dir.map(Path::to_path_buf).or_else(os_cache_dir);
        let mut loaded = HashMap::new();
        let mut uncompiled = HashMap::new();
        for dir in parser_dirs {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let Some(language) = name.strip_prefix("tree-sitter-") else {
                    continue;
                };
                if language.is_empty() {
                    continue;
                }
                match find_compiled_library(&path, language, cache_dir.as_deref()) {
                    Some(lib_path) => match load_grammar(language, &lib_path) {
                        Ok(grammar) => {
                            loaded.insert(language.to_string(), grammar);
                        }
                        Err(e) => {
                            log::warn!("{}", e.message);
                        }
                    },
                    None => {
                        uncompiled.insert(language.to_string(), path);
                    }
                }
            }
        }
        Ok(Self { loaded, uncompiled })
    }
}

#[derive(Debug, serde::Deserialize)]
struct QueryArgs {
    path: String,
    language: String,
    query: String,
    #[serde(default)]
    captures: Option<Vec<String>>,
    #[serde(default)]
    max_matches: Option<usize>,
    #[serde(default)]
    include_text: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct NodesArgs {
    path: String,
    language: String,
    node_type: String,
    #[serde(default)]
    max_matches: Option<usize>,
    #[serde(default)]
    include_text: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct RefsArgs {
    path: String,
    language: String,
    name: String,
    #[serde(default)]
    max_matches: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
struct TreeArgs {
    path: String,
    language: String,
    #[serde(default)]
    max_depth: Option<usize>,
}

/// An AST tool run against the process grammar stores.
///
/// The stores are a parameter, never a global: the composition root assembles
/// them once and the registration seam hands them to every AST call.
pub(crate) trait GrammarTool {
    const NAME: &'static str;

    fn run(
        args: &JsonValue,
        cwd: &Path,
        cancel: &CancelTx,
        stores: &GrammarStores,
    ) -> impl std::future::Future<Output = Result<JsonValue, ToolHandlerError>> + Send;
}

pub struct AstQueryTool;

impl GrammarTool for AstQueryTool {
    const NAME: &'static str = "ast_query";

    async fn run(
        args: &JsonValue,
        cwd: &Path,
        _cancel: &CancelTx,
        stores: &GrammarStores,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: QueryArgs = serde_json::from_value(args.clone()).map_err(|e| {
            ToolHandlerError::validation(format!("Invalid ast_query arguments: {e}"))
        })?;
        query_logic(stores, &args, cwd)
    }
}

pub struct AstNodesTool;

impl GrammarTool for AstNodesTool {
    const NAME: &'static str = "ast_nodes";

    async fn run(
        args: &JsonValue,
        cwd: &Path,
        _cancel: &CancelTx,
        stores: &GrammarStores,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: NodesArgs = serde_json::from_value(args.clone()).map_err(|e| {
            ToolHandlerError::validation(format!("Invalid ast_nodes arguments: {e}"))
        })?;
        nodes_logic(stores, &args, cwd)
    }
}

pub struct AstRefsTool;

impl GrammarTool for AstRefsTool {
    const NAME: &'static str = "ast_refs";

    async fn run(
        args: &JsonValue,
        cwd: &Path,
        _cancel: &CancelTx,
        stores: &GrammarStores,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: RefsArgs = serde_json::from_value(args.clone()).map_err(|e| {
            ToolHandlerError::validation(format!("Invalid ast_refs arguments: {e}"))
        })?;
        refs_logic(stores, &args, cwd)
    }
}

pub struct AstTreeTool;

impl GrammarTool for AstTreeTool {
    const NAME: &'static str = "ast_tree";

    async fn run(
        args: &JsonValue,
        cwd: &Path,
        _cancel: &CancelTx,
        stores: &GrammarStores,
    ) -> Result<JsonValue, ToolHandlerError> {
        let args: TreeArgs = serde_json::from_value(args.clone()).map_err(|e| {
            ToolHandlerError::validation(format!("Invalid ast_tree arguments: {e}"))
        })?;
        tree_logic(stores, &args, cwd)
    }
}

fn query_logic(
    stores: &GrammarStores,
    args: &QueryArgs,
    cwd: &Path,
) -> Result<JsonValue, ToolHandlerError> {
    let resolved = super::resolve_fs_path_for_cwd(&args.path, cwd);
    let language = args.language.as_str();
    let (source, tree) = stores.get_or_parse(&resolved, language)?;
    let grammar = stores.get_grammar(&resolved, language)?;
    let query = Query::new(&grammar.language, &args.query)
        .map_err(|e| ToolHandlerError::validation(format!("Invalid tree-sitter query: {e}")))?;
    let max_matches = args.max_matches.unwrap_or(100);
    let include_text = args.include_text.unwrap_or(true);
    let root = tree.root_node();
    let mut cursor = QueryCursor::new();
    let mut results = Vec::new();
    let mut total = 0;
    let mut matches_iter = cursor.matches(&query, root, source.as_bytes());
    while let Some(m) = matches_iter.next() {
        total += 1;
        if results.len() >= max_matches {
            continue;
        }
        let mut captures_map = serde_json::Map::new();
        for cap in m.captures {
            let cap_name = query.capture_names()[cap.index as usize].to_string();
            if let Some(filter) = &args.captures
                && !filter.contains(&cap_name)
            {
                continue;
            }
            let node = cap.node;
            let text = if include_text {
                serde_json::Value::String(
                    node.utf8_text(source.as_bytes()).unwrap_or("").to_string(),
                )
            } else {
                serde_json::Value::Null
            };
            let start = node.start_position();
            let end = node.end_position();
            captures_map.insert(
                cap_name,
                serde_json::json!({
                    "text": text,
                    "start_row": start.row,
                    "start_col": start.column,
                    "end_row": end.row,
                    "end_col": end.column,
                    "start_byte": node.start_byte(),
                    "end_byte": node.end_byte(),
                }),
            );
        }
        results.push(serde_json::json!({
            "pattern_index": m.pattern_index,
            "captures": captures_map,
        }));
    }
    let truncated = total > results.len();
    Ok(serde_json::json!({
        "file": args.path,
        "language": language,
        "matches": results,
        "truncated": truncated,
        "total_matches": total,
    }))
}

fn nodes_logic(
    stores: &GrammarStores,
    args: &NodesArgs,
    cwd: &Path,
) -> Result<JsonValue, ToolHandlerError> {
    let resolved = super::resolve_fs_path_for_cwd(&args.path, cwd);
    let language = args.language.as_str();
    let (source, tree) = stores.get_or_parse(&resolved, language)?;
    let max_results = args.max_matches.unwrap_or(200);
    let include_text = args.include_text.unwrap_or(false);
    let root = tree.root_node();
    let mut nodes = Vec::new();
    let mut total = 0;
    let mut queue = std::collections::VecDeque::from([root]);
    while let Some(node) = queue.pop_front() {
        if node.kind() == args.node_type {
            total += 1;
            if nodes.len() < max_results {
                let start = node.start_position();
                let end = node.end_position();
                let text = if include_text {
                    serde_json::Value::String(
                        node.utf8_text(source.as_bytes()).unwrap_or("").to_string(),
                    )
                } else {
                    serde_json::Value::Null
                };
                nodes.push(serde_json::json!({
                    "start_row": start.row,
                    "start_col": start.column,
                    "end_row": end.row,
                    "end_col": end.column,
                    "start_byte": node.start_byte(),
                    "end_byte": node.end_byte(),
                    "text": text,
                }));
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            queue.push_back(child);
        }
    }
    let truncated = total > nodes.len();
    Ok(serde_json::json!({
        "file": args.path,
        "language": language,
        "node_type": args.node_type,
        "nodes": nodes,
        "total": total,
        "truncated": truncated,
    }))
}

fn refs_logic(
    stores: &GrammarStores,
    args: &RefsArgs,
    cwd: &Path,
) -> Result<JsonValue, ToolHandlerError> {
    let resolved = super::resolve_fs_path_for_cwd(&args.path, cwd);
    let language = args.language.as_str();
    let (source, tree) = stores.get_or_parse(&resolved, language)?;
    let max_matches = args.max_matches.unwrap_or(100);
    let root = tree.root_node();
    let mut matches = Vec::new();
    let mut total = 0;
    let mut queue = std::collections::VecDeque::from([root]);
    while let Some(node) = queue.pop_front() {
        let kind = node.kind();
        if (kind == "identifier" || kind == "type_identifier")
            && node.utf8_text(source.as_bytes()).ok() == Some(args.name.as_str())
        {
            total += 1;
            if matches.len() >= max_matches {
                continue;
            }
            let start = node.start_position();
            let end = node.end_position();
            matches.push(serde_json::json!({
                "pattern_index": 0,
                "captures": {
                    "name": {
                        "text": args.name,
                        "start_row": start.row,
                        "start_col": start.column,
                        "end_row": end.row,
                        "end_col": end.column,
                        "start_byte": node.start_byte(),
                        "end_byte": node.end_byte(),
                    }
                }
            }));
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            queue.push_back(child);
        }
    }
    let truncated = total > matches.len();
    Ok(serde_json::json!({
        "file": args.path,
        "language": language,
        "matches": matches,
        "truncated": truncated,
        "total_matches": total,
    }))
}

fn tree_logic(
    stores: &GrammarStores,
    args: &TreeArgs,
    cwd: &Path,
) -> Result<JsonValue, ToolHandlerError> {
    let resolved = super::resolve_fs_path_for_cwd(&args.path, cwd);
    let language = args.language.as_str();
    let (source, tree) = stores.get_or_parse(&resolved, language)?;
    let root = tree.root_node();
    let sexp = match args.max_depth {
        Some(depth) => node_to_sexp(root, source.as_bytes(), depth, 0),
        None => root.to_sexp(),
    };
    Ok(serde_json::json!({
        "file": args.path,
        "language": language,
        "tree": sexp,
    }))
}

fn node_to_sexp(
    node: tree_sitter::Node,
    source: &[u8],
    max_depth: usize,
    current_depth: usize,
) -> String {
    if current_depth >= max_depth {
        return "...".to_string();
    }
    let kind = node.kind();
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    if children.is_empty() {
        if node.is_named() {
            format!("({kind})")
        } else {
            format!("\"{}\"", node.utf8_text(source).unwrap_or(""))
        }
    } else {
        let child_strs: Vec<String> = children
            .into_iter()
            .map(|c| node_to_sexp(c, source, max_depth, current_depth + 1))
            .collect();
        format!("({kind} {})", child_strs.join(" "))
    }
}

#[cfg(test)]
#[path = "../../../test/tools/handler/tree_sitter.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../test/tools/handler/tree_sitter_grammar/mod.rs"]
mod grammar_tests;
