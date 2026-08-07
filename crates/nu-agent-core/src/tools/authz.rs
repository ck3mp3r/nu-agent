use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value as JsonValue;

use async_trait::async_trait;

use crate::protocol::{
    event::{PermissionDecision as UiPermissionDecision, PermissionRequestContext, ToolDisplay},
    permission::{
        PermissionController, PermissionRequest, PermissionRequestToken, PermissionResolution,
    },
};

static NEXT_PERMISSION_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionAction {
    Allow,
    Ask,
    Deny,
}

impl PermissionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "allow" => Some(Self::Allow),
            "ask" => Some(Self::Ask),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskChoice {
    AllowOnce,
    AllowAlways,
    Deny,
}

impl From<UiPermissionDecision> for AskChoice {
    fn from(decision: UiPermissionDecision) -> Self {
        match decision {
            UiPermissionDecision::AllowOnce => AskChoice::AllowOnce,
            UiPermissionDecision::AllowAlways => AskChoice::AllowAlways,
            UiPermissionDecision::Deny => AskChoice::Deny,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonInteractiveAskMode {
    Deny,
    Allow,
}

#[derive(Debug, Clone, Copy)]
pub struct AskRuntimeConfig {
    pub interactive: bool,
    pub non_interactive_mode: NonInteractiveAskMode,
    pub timeout: Duration,
}

impl Default for AskRuntimeConfig {
    fn default() -> Self {
        Self {
            interactive: true,
            non_interactive_mode: NonInteractiveAskMode::Deny,
            timeout: Duration::from_secs(30),
        }
    }
}

pub trait PermissionEventSink: Send {
    fn emit(&mut self, event: crate::protocol::event::UiEvent);
}

#[derive(Debug, Clone, Default)]
pub struct AskContext {
    pub pre_authorize_display: Option<ToolDisplay>,
}

#[derive(Debug)]
pub struct AsyncAskHook {
    controller: PermissionController,
    config: AskRuntimeConfig,
    current_token: Option<PermissionRequestToken>,
    active_request_id: Option<String>,
}

impl AsyncAskHook {
    pub fn new(config: AskRuntimeConfig) -> Self {
        Self {
            controller: PermissionController::new(config.timeout),
            config,
            current_token: None,
            active_request_id: None,
        }
    }

    pub fn active_request_id(&self) -> Option<&str> {
        self.active_request_id.as_deref()
    }

    pub async fn choose_with_sink<S: PermissionEventSink + Send>(
        &mut self,
        decision: &PermissionDecision,
        tool_name: &str,
        source: &str,
        args: &JsonValue,
        ask_context: &AskContext,
        mut sink: Option<&mut S>,
    ) -> AskChoice {
        if !self.config.interactive {
            return match self.config.non_interactive_mode {
                NonInteractiveAskMode::Deny => AskChoice::Deny,
                NonInteractiveAskMode::Allow => AskChoice::AllowOnce,
            };
        }

        let request_id = next_request_id();
        let request_context = PermissionRequestContext {
            tool: display_tool_name(tool_name, args),
            source: source.to_string(),
            mode: args
                .get("mode")
                .and_then(JsonValue::as_str)
                .map(ToString::to_string),
            matched_rule_identity: decision.matched_rule.identity.clone(),
            scope: decision.matched_rule.scope.to_string(),
            target_field: decision.matched_rule.target_field.clone(),
            pattern: decision.matched_rule.pattern.clone(),
            summary: summarize_ask_payload(args),
            pre_authorize_display: ask_context.pre_authorize_display.clone(),
        };

        let (token, requested_event) = match self.controller.begin_request(PermissionRequest {
            request_id: request_id.clone(),
            context: request_context,
        }) {
            Ok(value) => value,
            Err(_) => return AskChoice::Deny,
        };

        crate::protocol::permission::install_active_permission_submission_sender(Some(
            token.sender_clone(),
        ));
        self.active_request_id = Some(request_id);
        self.current_token = Some(token);
        if let Some(s) = &mut sink {
            s.emit(requested_event);
        }

        let (resolution, events) = self
            .controller
            .await_resolution(self.current_token.as_ref().expect("token set"))
            .await;
        if let Some(s) = &mut sink {
            for event in events {
                s.emit(event);
            }
        }

        crate::protocol::permission::install_active_permission_submission_sender(None);
        self.current_token = None;
        self.active_request_id = None;

        match resolution {
            PermissionResolution::Decision { decision, .. } => AskChoice::from(decision),
            PermissionResolution::TimedOut | PermissionResolution::ChannelClosed => AskChoice::Deny,
        }
    }
}

fn next_request_id() -> String {
    format!(
        "ask-{:016x}",
        NEXT_PERMISSION_REQUEST_ID.fetch_add(1, Ordering::SeqCst)
    )
}

fn summarize_ask_payload(args: &JsonValue) -> String {
    let compact = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    let trimmed = if compact.chars().count() > 240 {
        let mut prefix = compact.chars().take(240).collect::<String>();
        prefix.push('…');
        prefix
    } else {
        compact
    };
    format!("→ {trimmed}")
}

/// Format tool name with arguments for display in permission prompts.
/// Returns `tool_name(arg1=val1, arg2=val2)` or just `tool_name` if no args.
/// Arguments are sorted alphabetically by key.
/// String values longer than 60 chars are truncated with `…`.
/// Null values are skipped.
pub fn display_tool_name(tool_name: &str, args: &JsonValue) -> String {
    // Only process object types
    let obj = match args.as_object() {
        Some(obj) => obj,
        None => return tool_name.to_string(),
    };

    // Collect non-null entries
    let mut entries: Vec<(String, String)> = Vec::new();
    for (key, value) in obj {
        if value.is_null() {
            continue;
        }

        let formatted_value = format_arg_value(value);
        entries.push((key.clone(), formatted_value));
    }

    // If no non-null entries, return just tool name
    if entries.is_empty() {
        return tool_name.to_string();
    }

    // Sort alphabetically by key
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Format as tool_name(key1=val1, key2=val2)
    let args_str = entries
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");

    format!("{tool_name}({args_str})")
}

/// Format a single argument value for display.
fn format_arg_value(value: &JsonValue) -> String {
    const MAX_LEN: usize = 60;

    match value {
        JsonValue::String(s) => {
            if s.chars().count() > MAX_LEN {
                let mut result = s.chars().take(MAX_LEN).collect::<String>();
                result.push('…');
                result
            } else {
                s.clone()
            }
        }
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Object(_) | JsonValue::Array(_) => {
            // Convert to compact JSON
            let json = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
            if json.chars().count() > MAX_LEN {
                let mut result = json.chars().take(MAX_LEN).collect::<String>();
                result.push('…');
                result
            } else {
                json
            }
        }
        JsonValue::Null => String::new(), // Should never reach here as nulls are filtered
    }
}

#[async_trait]
pub trait AskApprovalHook {
    async fn choose<S: PermissionEventSink + Send>(
        &mut self,
        decision: &PermissionDecision,
        tool_name: &str,
        source: &str,
        args: &JsonValue,
        ask_context: &AskContext,
        sink: Option<&mut S>,
    ) -> AskChoice;
}

#[async_trait]
impl AskApprovalHook for AsyncAskHook {
    async fn choose<S: PermissionEventSink + Send>(
        &mut self,
        decision: &PermissionDecision,
        tool_name: &str,
        source: &str,
        args: &JsonValue,
        ask_context: &AskContext,
        sink: Option<&mut S>,
    ) -> AskChoice {
        AsyncAskHook::choose_with_sink(self, decision, tool_name, source, args, ask_context, sink)
            .await
    }
}

#[async_trait]
impl AskApprovalHook for AutoApproveAskHook {
    async fn choose<S: PermissionEventSink + Send>(
        &mut self,
        _decision: &PermissionDecision,
        _tool_name: &str,
        _source: &str,
        _args: &JsonValue,
        _ask_context: &AskContext,
        _sink: Option<&mut S>,
    ) -> AskChoice {
        AskChoice::AllowOnce
    }
}

#[derive(Debug, Default)]
pub struct AutoApproveAskHook;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDiagnostic {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRuleMatch {
    pub identity: String,
    pub scope: &'static str,
    pub target_field: Option<String>,
    pub pattern: String,
    pub action: PermissionAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecision {
    pub action: PermissionAction,
    pub matched_rule: PermissionRuleMatch,
    pub diagnostics: Vec<PermissionDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct PermissionsConfig {
    global: PermissionAction,
    tool_rules: Vec<(String, PermissionAction)>,
    nested_field_rules: HashMap<String, HashMap<String, Vec<(String, PermissionAction)>>>,
    diagnostics: Vec<PermissionDiagnostic>,
}

#[derive(Debug, Clone, Default)]
pub struct PermissionsOverlay {
    global: Option<PermissionAction>,
    tool_rules: BTreeMap<String, PermissionAction>,
    nested_field_rules: HashMap<String, HashMap<String, Vec<(String, PermissionAction)>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionsSummary {
    pub global: PermissionAction,
    pub tool_rule_count: usize,
    pub nested_field_rule_count: usize,
}

impl PermissionsConfig {
    pub fn safe_defaults(interactive: bool) -> Self {
        let global = if interactive {
            PermissionAction::Ask
        } else {
            PermissionAction::Deny
        };
        Self {
            global,
            tool_rules: vec![
                ("read".to_string(), PermissionAction::Allow),
                ("glob".to_string(), PermissionAction::Allow),
                ("grep".to_string(), PermissionAction::Allow),
                ("c5t_get*".to_string(), PermissionAction::Allow),
                ("c5t_list*".to_string(), PermissionAction::Allow),
            ],
            nested_field_rules: HashMap::new(),
            diagnostics: vec![PermissionDiagnostic {
                code: "permissions.defaults.applied",
                message: "permissions block missing; using conservative defaults".to_string(),
            }],
        }
    }

    pub fn is_tool_visible(&self, tool_name: &str) -> bool {
        let decision = self.evaluate(tool_name, &serde_json::json!({}));
        decision.action != PermissionAction::Deny
    }

    pub fn parse_from_plugin_config(
        plugin_config: Option<&nu_protocol::Value>,
        interactive: bool,
    ) -> Self {
        let Some(value) = plugin_config else {
            return Self::safe_defaults(interactive);
        };
        let Ok(record) = value.as_record() else {
            return Self::safe_defaults(interactive);
        };
        let Some(permissions_value) = record.get("permissions") else {
            return Self::safe_defaults(interactive);
        };

        let mut diagnostics = Vec::new();
        let mut global = PermissionAction::Ask;
        let mut tool_rules_map = BTreeMap::<String, PermissionAction>::new();
        let mut nested_field_rules_map: HashMap<
            String,
            HashMap<String, Vec<(String, PermissionAction)>>,
        > = HashMap::new();

        let Ok(permissions_record) = permissions_value.as_record() else {
            return Self::safe_defaults(interactive);
        };

        for (tool_key, tool_value) in permissions_record.iter() {
            if tool_key == "*" {
                match value_to_action(tool_value) {
                    Ok(action) => global = action,
                    Err(message) => diagnostics.push(PermissionDiagnostic {
                        code: "permissions.invalid.global_action",
                        message,
                    }),
                }
                continue;
            }

            let resolved_key = tool_key.as_str();

            // If value is a record, treat as nested field → pattern → action rules
            if let Ok(field_record) = tool_value.as_record() {
                for (field_name, field_value) in field_record.iter() {
                    let Ok(pattern_record) = field_value.as_record() else {
                        diagnostics.push(PermissionDiagnostic {
                            code: "permissions.invalid.nested_field_map",
                            message: format!(
                                "permissions.{resolved_key}.{field_name} must be a map of pattern -> action"
                            ),
                        });
                        continue;
                    };
                    for (pattern, action_value) in pattern_record.iter() {
                        match value_to_action(action_value) {
                            Ok(action) => {
                                nested_field_rules_map
                                    .entry(resolved_key.to_string())
                                    .or_default()
                                    .entry(field_name.to_string())
                                    .or_default()
                                    .push((pattern.to_string(), action));
                            }
                            Err(message) => diagnostics.push(PermissionDiagnostic {
                                code: "permissions.invalid.nested_field_action",
                                message: format!("{message} for pattern '{pattern}'"),
                            }),
                        }
                    }
                }
                continue;
            }

            match value_to_action(tool_value) {
                Ok(action) => {
                    tool_rules_map.insert(resolved_key.to_string(), action);
                }
                Err(message) => diagnostics.push(PermissionDiagnostic {
                    code: "permissions.invalid.tool_action",
                    message: format!("{message} for key '{resolved_key}'"),
                }),
            }
        }

        Self {
            global,
            tool_rules: tool_rules_map.into_iter().collect(),
            nested_field_rules: nested_field_rules_map,
            diagnostics,
        }
    }
    pub fn evaluate(&self, tool_name: &str, args: &JsonValue) -> PermissionDecision {
        let mut diagnostics = self.diagnostics.clone();

        let mut matched_rule = PermissionRuleMatch {
            identity: "global:*".to_string(),
            scope: "global",
            target_field: None,
            pattern: "*".to_string(),
            action: self.global,
        };

        if let Some((pattern, action)) = match_pattern(tool_name, &self.tool_rules) {
            matched_rule = PermissionRuleMatch {
                identity: format!("tool:{pattern}"),
                scope: "tool",
                target_field: None,
                pattern: pattern.to_string(),
                action,
            };
        }

        if let Some(field_rules) = self.nested_field_rules.get(tool_name) {
            let inherited = matched_rule.action;
            for (field_name, rules) in field_rules {
                let Some(raw_value) = args.get(field_name.as_str()).and_then(|v| v.as_str()) else {
                    diagnostics.push(PermissionDiagnostic {
                        code: "permissions.nested_field.missing",
                        message: format!(
                            "nested field '{field_name}' for tool '{tool_name}' missing or unreadable; \
                             using inherited decision"
                        ),
                    });
                    continue;
                };
                let normalized = raw_value.split_whitespace().collect::<Vec<_>>().join(" ");
                if let Some((pattern, action)) = match_pattern(normalized.as_str(), rules) {
                    if pattern == "*" && action == inherited {
                        diagnostics.push(PermissionDiagnostic {
                            code: "permissions.noop.nested_field.star",
                            message: format!(
                                "{tool_name}.{field_name} '*' matched but inherited decision is unchanged"
                            ),
                        });
                    }
                    matched_rule = PermissionRuleMatch {
                        identity: format!("nested:{tool_name}.{field_name}:{pattern}"),
                        scope: "nested",
                        target_field: Some(field_name.clone()),
                        pattern: pattern.to_string(),
                        action,
                    };
                }
            }
        }

        PermissionDecision {
            action: matched_rule.action,
            matched_rule,
            diagnostics,
        }
    }
    pub fn with_overlay(&self, overlay: &PermissionsOverlay) -> Self {
        let mut merged_tool_rules: BTreeMap<String, PermissionAction> =
            self.tool_rules.iter().cloned().collect();

        for (pattern, action) in &overlay.tool_rules {
            merged_tool_rules.insert(pattern.clone(), *action);
        }

        // Two-level merge: overlay wins on pattern overlap per (tool, field)
        let mut merged_nested = self.nested_field_rules.clone();
        for (tool_name, field_rules) in &overlay.nested_field_rules {
            let tool_entry = merged_nested.entry(tool_name.clone()).or_default();
            for (field_name, overlay_rules) in field_rules {
                let mut merged_rules: BTreeMap<String, PermissionAction> = tool_entry
                    .get(field_name)
                    .map(|base| base.iter().cloned().collect())
                    .unwrap_or_default();
                for (pattern, action) in overlay_rules {
                    merged_rules.insert(pattern.clone(), *action);
                }
                tool_entry.insert(field_name.clone(), merged_rules.into_iter().collect());
            }
        }

        Self {
            global: overlay.global.unwrap_or(self.global),
            tool_rules: merged_tool_rules.into_iter().collect(),
            nested_field_rules: merged_nested,
            diagnostics: self.diagnostics.clone(),
        }
    }

    pub fn summary(&self) -> PermissionsSummary {
        PermissionsSummary {
            global: self.global,
            tool_rule_count: self.tool_rules.len(),
            nested_field_rule_count: self
                .nested_field_rules
                .values()
                .flat_map(|f| f.values())
                .map(|r| r.len())
                .sum(),
        }
    }
}

impl PermissionsOverlay {
    pub fn parse_from_cli_value(value: &nu_protocol::Value) -> Result<Self, String> {
        let permissions_record = value
            .as_record()
            .map_err(|_| "permissions must be a record/object (path: permissions)".to_string())?;

        let mut overlay = Self::default();

        for (tool_key, tool_value) in permissions_record.iter() {
            if tool_key == "*" {
                let action = value_to_action(tool_value)
                    .map_err(|msg| format!("{msg} (path: permissions.*)"))?;
                overlay.global = Some(action);
                continue;
            }

            // If value is a record, treat as nested field \u{2192} pattern \u{2192} action rules
            if let Ok(field_record) = tool_value.as_record() {
                for (field_name, field_value) in field_record.iter() {
                    let Ok(pattern_record) = field_value.as_record() else {
                        return Err(format!(
                            "permissions.{}.{} value must be a map of pattern -> action (path: permissions.{}.{})",
                            path_segment(tool_key),
                            field_name,
                            path_segment(tool_key),
                            field_name
                        ));
                    };
                    for (pattern, action_value) in pattern_record.iter() {
                        let action = value_to_action(action_value).map_err(|msg| {
                            format!(
                                "{msg} (path: permissions.{}.{}.{})",
                                path_segment(tool_key),
                                field_name,
                                pattern
                            )
                        })?;
                        overlay
                            .nested_field_rules
                            .entry(tool_key.to_string())
                            .or_default()
                            .entry(field_name.to_string())
                            .or_default()
                            .push((pattern.to_string(), action));
                    }
                }
                continue;
            }

            let action = value_to_action(tool_value)
                .map_err(|msg| format!("{msg} (path: permissions.{})", path_segment(tool_key)))?;
            overlay.tool_rules.insert(tool_key.to_string(), action);
        }

        Ok(overlay)
    }

    pub fn parse_from_yaml(mapping: &noyalib::Mapping) -> Result<Self, String> {
        let mut overlay = Self::default();

        for (yaml_key, yaml_value) in mapping.iter() {
            let key_str = yaml_key.as_str();

            if key_str == "*" {
                let action_str = yaml_value.as_str().ok_or_else(|| {
                    "permissions.* value must be a string (path: permissions.*)".to_string()
                })?;
                let action = PermissionAction::from_str(action_str).ok_or_else(|| {
                    format!(
                        "invalid permission action '{}' (path: permissions.*)",
                        action_str
                    )
                })?;
                overlay.global = Some(action);
                continue;
            }

            // If value is a mapping, treat as nested field \u{2192} pattern \u{2192} action rules
            if let Some(field_mapping) = yaml_value.as_mapping() {
                for (field_key, field_value) in field_mapping.iter() {
                    let field_name = field_key.as_str();
                    let Some(pattern_mapping) = field_value.as_mapping() else {
                        return Err(format!(
                            "permissions.{}.{} value must be a map of pattern -> action (path: permissions.{}.{})",
                            key_str, field_name, key_str, field_name
                        ));
                    };
                    for (pattern_key, action_value) in pattern_mapping.iter() {
                        let pattern = pattern_key.as_str();
                        let action_str = action_value.as_str().ok_or_else(|| {
                            format!(
                                "permissions.{}.{} value must be a string (path: permissions.{}.{}.{})",
                                key_str, field_name, key_str, field_name, pattern
                            )
                        })?;
                        let action = PermissionAction::from_str(action_str).ok_or_else(|| {
                            format!(
                                "invalid permission action '{}' (path: permissions.{}.{}.{})",
                                action_str, key_str, field_name, pattern
                            )
                        })?;
                        overlay
                            .nested_field_rules
                            .entry(key_str.to_string())
                            .or_default()
                            .entry(field_name.to_string())
                            .or_default()
                            .push((pattern.to_string(), action));
                    }
                }
                continue;
            }

            let action_str = yaml_value.as_str().ok_or_else(|| {
                format!(
                    "permissions.{} value must be a string (path: permissions.{})",
                    key_str,
                    path_segment(key_str)
                )
            })?;

            let action = PermissionAction::from_str(action_str).ok_or_else(|| {
                format!(
                    "invalid permission action '{}' (path: permissions.{})",
                    action_str,
                    path_segment(key_str)
                )
            })?;
            overlay.tool_rules.insert(key_str.to_string(), action);
        }

        Ok(overlay)
    }
}

#[derive(Debug, Default, Clone)]
pub struct SessionGrantCache {
    grants_by_scope: HashMap<SessionGrantScopedKey, PermissionAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionGrantScopedKey {
    matched_rule_identity: String,
    tool_name: String,
    source: String,
    mode: Option<String>,
    target_field: Option<String>,
}

impl SessionGrantScopedKey {
    /// Deterministic cache key for allow_always session grants.
    ///
    /// Scope fields intentionally include matched rule identity *and* request context to prevent
    /// grant leakage across unrelated tools (for example when broad global ask rules match).
    fn from_request(
        decision: &PermissionDecision,
        tool_name: &str,
        source: &str,
        args: &JsonValue,
    ) -> Self {
        Self {
            matched_rule_identity: decision.matched_rule.identity.clone(),
            tool_name: tool_name.to_string(),
            source: source.to_string(),
            mode: args
                .get("mode")
                .and_then(JsonValue::as_str)
                .map(ToString::to_string),
            target_field: decision.matched_rule.target_field.clone(),
        }
    }
}

impl SessionGrantCache {
    pub fn get(
        &self,
        decision: &PermissionDecision,
        tool_name: &str,
        source: &str,
        args: &JsonValue,
    ) -> Option<PermissionAction> {
        let key = SessionGrantScopedKey::from_request(decision, tool_name, source, args);
        self.grants_by_scope.get(&key).copied()
    }

    pub fn insert_allow_always(
        &mut self,
        decision: &PermissionDecision,
        tool_name: &str,
        source: &str,
        args: &JsonValue,
    ) {
        let key = SessionGrantScopedKey::from_request(decision, tool_name, source, args);
        self.grants_by_scope.insert(key, PermissionAction::Allow);
    }

    /// Clear all session grants. Used when switching agents so that
    /// "Allow always" grants from the previous agent do not persist.
    pub fn clear(&mut self) {
        self.grants_by_scope.clear();
    }

    /// Clear session grants for tools belonging to a specific MCP server.
    /// Used when disabling an MCP server so that its tool grants are revoked.
    /// MCP tool names use the format `{server_name}__{tool_name}`.
    pub fn clear_for_server(&mut self, server_name: &str) {
        let prefix = format!("{}__", server_name);
        self.grants_by_scope
            .retain(|key, _| !key.tool_name.starts_with(&prefix));
    }
}

pub fn apply_ask_choice(
    decision: PermissionDecision,
    choice: AskChoice,
    grant_cache: &mut SessionGrantCache,
    tool_name: &str,
    source: &str,
    args: &JsonValue,
) -> PermissionDecision {
    match choice {
        AskChoice::AllowOnce => PermissionDecision {
            action: PermissionAction::Allow,
            ..decision
        },
        AskChoice::AllowAlways => {
            grant_cache.insert_allow_always(&decision, tool_name, source, args);
            PermissionDecision {
                action: PermissionAction::Allow,
                ..decision
            }
        }
        AskChoice::Deny => PermissionDecision {
            action: PermissionAction::Deny,
            ..decision
        },
    }
}

pub fn apply_session_grant_override(
    decision: PermissionDecision,
    grant_cache: &SessionGrantCache,
    tool_name: &str,
    source: &str,
    args: &JsonValue,
) -> PermissionDecision {
    if let Some(action) = grant_cache.get(&decision, tool_name, source, args) {
        return PermissionDecision { action, ..decision };
    }
    decision
}

fn value_to_action(value: &nu_protocol::Value) -> Result<PermissionAction, String> {
    let action_str = value
        .as_str()
        .map_err(|_| "permission action must be a string".to_string())?;
    PermissionAction::from_str(action_str)
        .ok_or_else(|| format!("invalid permission action '{}'", action_str))
}

fn pattern_specificity(pattern: &str) -> usize {
    pattern.chars().filter(|ch| *ch != '*').count()
}

fn match_pattern<'a>(
    candidate: &str,
    rules: &'a [(String, PermissionAction)],
) -> Option<(&'a str, PermissionAction)> {
    rules
        .iter()
        .filter_map(|(pattern, action)| {
            if glob_match(pattern.as_str(), candidate) {
                Some((pattern.as_str(), *action))
            } else {
                None
            }
        })
        .max_by_key(|(pattern, _)| pattern_specificity(pattern))
}

fn glob_match(pattern: &str, input: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let mut pat_rest = pattern;
    let mut input_rest = input;
    let mut backtrack_pat = None;
    let mut backtrack_input = None;

    while !input_rest.is_empty() {
        if let Some(stripped) = pat_rest.strip_prefix('*') {
            pat_rest = stripped;
            backtrack_pat = Some(pat_rest);
            backtrack_input = Some(input_rest);
            continue;
        }

        if let Some((p_ch, p_next)) = split_first_char(pat_rest)
            && let Some((i_ch, i_next)) = split_first_char(input_rest)
            && p_ch == i_ch
        {
            pat_rest = p_next;
            input_rest = i_next;
            continue;
        }

        if let (Some(saved_pat), Some(saved_input)) = (backtrack_pat, backtrack_input)
            && let Some((_, next_input)) = split_first_char(saved_input)
        {
            pat_rest = saved_pat;
            input_rest = next_input;
            backtrack_input = Some(next_input);
            continue;
        }

        return false;
    }

    while let Some(stripped) = pat_rest.strip_prefix('*') {
        pat_rest = stripped;
    }
    pat_rest.is_empty()
}

fn split_first_char(input: &str) -> Option<(char, &str)> {
    let mut chars = input.chars();
    let first = chars.next()?;
    Some((first, chars.as_str()))
}

fn path_segment(input: &str) -> String {
    input.replace('.', "\\.")
}

#[cfg(test)]
#[path = "authz_test.rs"]
mod authz_test;
