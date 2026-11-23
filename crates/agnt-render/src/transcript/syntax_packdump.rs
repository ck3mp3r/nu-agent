//! The curated syntax packdump asset and the runtime language-set seam.
//!
//! The renderer resolves a language hint against one process-wide [`SyntaxSet`].
//! Building that set from YAML costs most of a second, so the default set ships
//! as a checked-in packdump asset (uncompressed bincode) and `init` replaces it
//! once at startup when the user configured a language list.

use syntect::{
    dumps,
    parsing::{SyntaxDefinition, SyntaxSet, SyntaxSetBuilder},
};

use super::highlight::{SYNTAX_SET, resolve_syntax_hint};

// region:    --- Types

/// The languages the checked-in packdump asset resolves.
///
/// The asset carries every stock syntax; anything outside this list is marked
/// hidden, so a `.md` or `.txt` hint renders plain tokens. Every entry resolves
/// on the stock syntect set except `ts` and `toml`; see `docs/content/docs/user/configuration.md`.
pub(crate) const DEFAULT_LANGUAGES: &[&str] = &[
    "rust", "python", "go", "js", "ts", "json", "yaml", "toml", "bash", "sql", "c", "cpp", "ruby",
    "diff", "nu", "nix",
];

/// Which syntax set the renderer resolves hints against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxSetSpec {
    /// The checked-in curated packdump asset.
    Default,
    /// Every syntax the stock syntect set carries, plus the inline nu and nix
    /// definitions.
    All,
    /// Exactly the listed languages, resolved against the stock set.
    Languages(Vec<String>),
}

// endregion: --- Types

// region:    --- Impls

impl SyntaxSetSpec {
    /// The spec a `highlight_languages` config value names.
    ///
    /// `None` and an empty list both mean the default set, the single entry
    /// `all` means every stock syntax, and any other list is taken verbatim.
    pub fn from_config(languages: Option<&[String]>) -> Self {
        let Some(languages) = languages else {
            return Self::Default;
        };
        if languages.is_empty() {
            return Self::Default;
        }
        if languages.len() == 1 && languages[0] == "all" {
            return Self::All;
        }
        Self::Languages(languages.to_vec())
    }
}

// endregion: --- Impls

// region:    --- Functions

/// Install the process-wide syntax set. The first call wins; later calls leave
/// the installed set unchanged.
pub fn init(spec: SyntaxSetSpec) {
    if SYNTAX_SET.get().is_some() {
        log::debug!("highlight syntax set already initialised; keeping it");
        return;
    }
    log::debug!("initialising the highlight syntax set: {spec:?}");
    let _ = SYNTAX_SET.set(build_syntax_set(&spec));
}

/// The syntax set `spec` names.
pub(crate) fn build_syntax_set(spec: &SyntaxSetSpec) -> SyntaxSet {
    match spec {
        SyntaxSetSpec::Default => default_syntax_set(),
        SyntaxSetSpec::All => stock_syntax_set(),
        SyntaxSetSpec::Languages(languages) => {
            let entries = languages.iter().map(String::as_str).collect::<Vec<_>>();
            restrict_syntax_set(stock_syntax_set(), &entries)
        }
    }
}

/// The set the checked-in asset carries: the stock set restricted to the
/// default language list.
pub(crate) fn curated_syntax_set() -> SyntaxSet {
    restrict_syntax_set(stock_syntax_set(), DEFAULT_LANGUAGES)
}

/// The checked-in curated packdump asset.
///
/// A corrupt asset falls back to the same set the regen path writes.
pub(crate) fn default_syntax_set() -> SyntaxSet {
    match dumps::from_uncompressed_data::<SyntaxSet>(DEFAULT_PACKDUMP) {
        Ok(syntax_set) => syntax_set,
        Err(err) => {
            log::warn!(
                "could not load the highlight syntax packdump: {err}; rebuilding it from the stock set"
            );
            curated_syntax_set()
        }
    }
}

/// The stock syntect set plus the inline nu and nix definitions.
pub(crate) fn stock_syntax_set() -> SyntaxSet {
    let stock = SyntaxSet::load_defaults_newlines();
    let needs_nix = stock.find_syntax_by_extension("nix").is_none();
    let mut builder = stock.into_builder();
    if let Some(nu_syntax) = inline_syntax(NU_SYNTAX, "Nu") {
        builder.add(nu_syntax);
    }
    if needs_nix && let Some(nix_syntax) = inline_syntax(NIX_SYNTAX, "Nix") {
        builder.add(nix_syntax);
    }
    builder.build()
}

// endregion: --- Functions

// region:    --- Support

/// The checked-in packdump asset, read at compile time.
const DEFAULT_PACKDUMP: &[u8] = include_bytes!("../assets/default.packdump");

const NU_SYNTAX: &str = include_str!("../assets/nu.sublime-syntax");

const NIX_SYNTAX: &str = include_str!("../assets/nix.sublime-syntax");

/// One inline syntax definition, or `None` when syntect rejects it.
fn inline_syntax(source: &str, name: &str) -> Option<SyntaxDefinition> {
    match SyntaxDefinition::load_from_str(source, true, Some(name)) {
        Ok(syntax) => Some(syntax),
        Err(err) => {
            log::warn!("could not load the inline {name} syntax: {err:?}");
            None
        }
    }
}

/// The stock set with every unlisted syntax marked hidden.
///
/// Dropping a syntax is not expressible: `SyntaxSet::into_builder` hands back
/// definitions whose contexts already carry `Direct` ids into the original
/// syntax indices, so a builder with fewer or reordered syntaxes fails to link.
/// Marking the rest hidden keeps every index in place and makes the resolver
/// skip them, which is the same observable set.
fn restrict_syntax_set(stock: SyntaxSet, languages: &[&str]) -> SyntaxSet {
    let mut listed = Vec::new();
    for language in languages {
        match resolve_syntax_hint(&stock, language) {
            Some(syntax) => listed.push(syntax.name.clone()),
            None => {
                log::debug!("highlight language {language:?} is not in the stock set; skipping it")
            }
        }
    }

    let source = stock.into_builder();
    let mut builder = SyntaxSetBuilder::new();
    for mut definition in source.syntaxes().to_vec() {
        if !listed.contains(&definition.name) {
            definition.hidden = true;
        }
        builder.add(definition);
    }
    builder.build()
}

// endregion: --- Support
