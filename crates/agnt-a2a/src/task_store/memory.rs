use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use chrono::Utc;
use tokio::sync::mpsc;
use uuid::Uuid;

use serde_json::Value;

use crate::{
    A2aError, Artifact, Message, Part, PushAuthScheme, PushAuthenticationInfo,
    PushNotificationConfig, Role, Task, TaskEvent, TaskState, TaskStatus,
};

use super::TaskStoreBackend;

/// A thread-safe in-memory store for A2A tasks.
///
/// All operations are synchronous — `std::sync::RwLock` is used rather than
/// tokio's async variant because every operation is a simple HashMap lookup.
#[derive(Debug)]
pub struct InMemoryTaskStore {
    tasks: RwLock<HashMap<String, Task>>,
    subscriptions: RwLock<HashMap<String, Vec<mpsc::Sender<TaskEvent>>>>,
    push_configs: RwLock<HashMap<String, Vec<PushNotificationConfig>>>,
    idempotency_keys: RwLock<HashMap<String, String>>,
    update_seq: RwLock<HashMap<String, u64>>,
    next_seq: AtomicU64,
    /// The transport push deliveries ride. `None` on a store built without one:
    /// push delivery is then skipped with a warning.
    transport: Option<reqwest::Client>,
}

impl Default for InMemoryTaskStore {
    fn default() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            subscriptions: RwLock::new(HashMap::new()),
            push_configs: RwLock::new(HashMap::new()),
            idempotency_keys: RwLock::new(HashMap::new()),
            update_seq: RwLock::new(HashMap::new()),
            next_seq: AtomicU64::new(0),
            transport: None,
        }
    }
}

impl InMemoryTaskStore {
    /// Store whose push deliveries ride `transport`.
    ///
    /// The transport is the composition root's `reqwest::Client`. A store
    /// built with [`Default`] has none, so it skips push delivery with a
    /// warning.
    pub fn with_transport(transport: reqwest::Client) -> Self {
        Self {
            transport: Some(transport),
            ..Self::default()
        }
    }
}

impl InMemoryTaskStore {
    // -----------------------------------------------------------------------
    // Lock accessors
    // -----------------------------------------------------------------------

    /// The task map, recovered if a writer panicked mid-insert.
    fn read_tasks(&self) -> RwLockReadGuard<'_, HashMap<String, Task>> {
        self.tasks.read().unwrap_or_else(|e| e.into_inner())
    }

    /// The task map, recovered if a reader panicked mid-read.
    fn write_tasks(&self) -> RwLockWriteGuard<'_, HashMap<String, Task>> {
        self.tasks.write().unwrap_or_else(|e| e.into_inner())
    }

    /// The subscription map, recovered if a writer panicked mid-insert.
    fn read_subscriptions(
        &self,
    ) -> RwLockReadGuard<'_, HashMap<String, Vec<mpsc::Sender<TaskEvent>>>> {
        self.subscriptions.read().unwrap_or_else(|e| e.into_inner())
    }

    /// The subscription map, recovered if a reader panicked mid-read.
    fn write_subscriptions(
        &self,
    ) -> RwLockWriteGuard<'_, HashMap<String, Vec<mpsc::Sender<TaskEvent>>>> {
        self.subscriptions
            .write()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// The push-config map, recovered if a writer panicked mid-insert.
    fn read_push_configs(
        &self,
    ) -> RwLockReadGuard<'_, HashMap<String, Vec<PushNotificationConfig>>> {
        self.push_configs.read().unwrap_or_else(|e| e.into_inner())
    }

    /// The push-config map, recovered if a reader panicked mid-read.
    fn write_push_configs(
        &self,
    ) -> RwLockWriteGuard<'_, HashMap<String, Vec<PushNotificationConfig>>> {
        self.push_configs.write().unwrap_or_else(|e| e.into_inner())
    }

    /// The idempotency map, recovered if a writer panicked mid-insert.
    fn write_idempotency_keys(&self) -> RwLockWriteGuard<'_, HashMap<String, String>> {
        self.idempotency_keys
            .write()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// The per-task update sequence, recovered if a writer panicked mid-insert.
    fn read_update_seq(&self) -> RwLockReadGuard<'_, HashMap<String, u64>> {
        self.update_seq.read().unwrap_or_else(|e| e.into_inner())
    }

    /// The per-task update sequence, recovered if a reader panicked mid-read.
    fn write_update_seq(&self) -> RwLockWriteGuard<'_, HashMap<String, u64>> {
        self.update_seq.write().unwrap_or_else(|e| e.into_inner())
    }
}

impl TaskStoreBackend for InMemoryTaskStore {
    fn create_task(
        &self,
        context_id: Option<String>,
        parent_task_id: Option<String>,
        metadata: Option<HashMap<String, Value>>,
    ) -> Task {
        InMemoryTaskStore::create_task(self, context_id, parent_task_id, metadata)
    }

    fn get_task(&self, id: &str) -> Result<Task, A2aError> {
        InMemoryTaskStore::get_task(self, id)
    }

    fn update_status(
        &self,
        id: &str,
        new_state: TaskState,
        message: Option<Message>,
    ) -> Result<Task, A2aError> {
        InMemoryTaskStore::update_status(self, id, new_state, message)
    }

    fn add_artifact(&self, id: &str, artifact: Artifact) -> Result<Task, A2aError> {
        InMemoryTaskStore::add_artifact(self, id, artifact)
    }

    fn list_tasks(
        &self,
        filter: Option<Vec<TaskState>>,
        page_size: Option<usize>,
        next_page_token: Option<&str>,
    ) -> (Vec<Task>, usize, Option<String>) {
        let limit = page_size.unwrap_or(50);
        let all = InMemoryTaskStore::list_tasks(self, None);

        // Apply state filter if provided (supports multiple states)
        let filtered: Vec<Task> = if let Some(states) = filter {
            all.into_iter()
                .filter(|t| states.contains(&t.status.state))
                .collect()
        } else {
            all
        };
        let filtered_total = filtered.len();

        // Sort by status timestamp descending (spec §3.1.4), then by the
        // store-assigned update sequence descending, then by task ID ascending
        // so equal timestamps still order deterministically (HashMap iteration
        // order is not stable).
        let seq = self.read_update_seq();
        let mut sorted = filtered;
        sorted.sort_by(|a, b| {
            b.status
                .timestamp
                .cmp(&a.status.timestamp)
                .then_with(|| seq.get(&b.id).cmp(&seq.get(&a.id)))
                .then_with(|| a.id.cmp(&b.id))
        });

        // Apply cursor-based pagination
        if let Some(cursor_id) = next_page_token
            && let Some(pos) = sorted.iter().position(|t| t.id == cursor_id)
        {
            sorted = sorted.split_off(pos + 1);
        }

        // Apply limit and determine if there are more results
        let has_more = sorted.len() > limit;
        sorted.truncate(limit);

        let next_token = if has_more {
            sorted.last().map(|t| t.id.clone())
        } else {
            None
        };

        (sorted, filtered_total, next_token)
    }

    fn subscribe(&self, id: &str) -> mpsc::Receiver<TaskEvent> {
        let (rx, _) = InMemoryTaskStore::subscribe(self, id);
        rx
    }

    fn unregister_subscriber(&self, id: &str, rx: mpsc::Receiver<TaskEvent>) {
        // Dropping the receiver closes the corresponding sender(s).
        drop(rx);
        self.prune_subscriptions(id);
    }

    fn append_history(&self, id: &str, msg: Message) -> Result<(), A2aError> {
        InMemoryTaskStore::append_history(self, id, msg).map(|_| ())
    }

    fn create_task_with_idempotency(
        &self,
        key: &str,
        context_id: Option<String>,
        parent_task_id: Option<String>,
        metadata: Option<HashMap<String, Value>>,
    ) -> Result<Task, Box<(Task, mpsc::Sender<TaskEvent>)>> {
        let (tx, _rx) = mpsc::channel(64);

        match InMemoryTaskStore::create_task_with_idempotency_impl(
            self,
            key,
            context_id,
            parent_task_id,
            metadata,
        ) {
            Ok(task) => {
                self.write_subscriptions()
                    .entry(task.id.clone())
                    .or_default()
                    .push(tx.clone());
                Ok(task)
            }
            Err(existing) => {
                self.write_subscriptions()
                    .entry(existing.id.clone())
                    .or_default()
                    .push(tx.clone());
                Err(Box::new((*existing, tx)))
            }
        }
    }
}

// -----------------------------------------------------------------------
// Internal helper — separates idempotency logic from subscription setup
// so the trait impl can add subscription wrapping without re-entrancy.
// -----------------------------------------------------------------------
impl InMemoryTaskStore {
    fn create_task_with_idempotency_impl(
        &self,
        key: &str,
        context_id: Option<String>,
        parent_task_id: Option<String>,
        metadata: Option<HashMap<String, Value>>,
    ) -> Result<Task, Box<Task>> {
        let mut keys = self.write_idempotency_keys();
        if let Some(existing_id) = keys.get(key)
            && let Ok(task) = self.get_task(existing_id)
        {
            return Err(Box::new(task)); // Found duplicate
        }

        let task = self.create_task(context_id, parent_task_id, metadata);
        keys.insert(key.to_string(), task.id.clone());
        Ok(task)
    }
}

impl InMemoryTaskStore {
    // -----------------------------------------------------------------------
    // Subscriptions
    // -----------------------------------------------------------------------

    /// Subscribe to task events for a given task ID.
    ///
    /// Returns a receiver and whether the task already exists in the store.
    pub fn subscribe(&self, task_id: &str) -> (mpsc::Receiver<TaskEvent>, bool) {
        let (tx, rx) = mpsc::channel(64);
        let mut subs = self.write_subscriptions();
        subs.entry(task_id.to_string()).or_default().push(tx);
        let exists = self.read_tasks().contains_key(task_id);
        (rx, exists)
    }

    /// Remove all senders for a task that have been dropped (cleanup).
    ///
    /// Called internally when sending to a subscriber fails.
    fn prune_subscriptions(&self, task_id: &str) {
        let mut subs = self.write_subscriptions();
        if let Some(senders) = subs.get_mut(task_id) {
            senders.retain(|tx| !tx.is_closed());
            if senders.is_empty() {
                subs.remove(task_id);
            }
        }
    }

    /// Notify all subscribers of a status change.
    fn notify_status_change(&self, task_id: &str, status: &TaskStatus) {
        let subs = self.read_subscriptions();
        if let Some(senders) = subs.get(task_id) {
            let event = TaskEvent::StatusChanged {
                task_id: task_id.to_string(),
                status: status.clone(),
            };
            let mut failed = false;
            for tx in senders {
                if let Err(e) = tx.try_send(event.clone()) {
                    log::warn!("subscription channel full for task {task_id}: {e}");
                    failed = true;
                }
            }
            // Drop read lock before pruning (writes need exclusive access)
            drop(subs);
            if failed {
                self.prune_subscriptions(task_id);
            }
        }
        self.notify_push_configs(
            task_id,
            &TaskEvent::StatusChanged {
                task_id: task_id.to_string(),
                status: status.clone(),
            },
        );
    }

    /// Notify all subscribers of a new artifact.
    fn notify_artifact_added(&self, task_id: &str, artifact: &Artifact) {
        let subs = self.read_subscriptions();
        if let Some(senders) = subs.get(task_id) {
            let event = TaskEvent::ArtifactAdded {
                task_id: task_id.to_string(),
                artifact: artifact.clone(),
            };
            let mut failed = false;
            for tx in senders {
                if let Err(e) = tx.try_send(event.clone()) {
                    log::warn!("subscription channel full for task {task_id}: {e}");
                    failed = true;
                }
            }
            drop(subs);
            if failed {
                self.prune_subscriptions(task_id);
            }
        }
        self.notify_push_configs(
            task_id,
            &TaskEvent::ArtifactAdded {
                task_id: task_id.to_string(),
                artifact: artifact.clone(),
            },
        );
    }

    // -----------------------------------------------------------------------
    // Push notification configs
    // -----------------------------------------------------------------------

    /// Register a new push notification webhook for a task.
    pub fn create_push_config(
        &self,
        task_id: &str,
        url: &str,
        authentication: Option<PushAuthenticationInfo>,
    ) -> PushNotificationConfig {
        let mut configs = self.write_push_configs();
        let config = PushNotificationConfig {
            id: Uuid::new_v4().to_string(),
            url: url.to_string(),
            task_id: task_id.to_string(),
            authentication,
            created_at: Utc::now(),
        };
        configs
            .entry(task_id.to_string())
            .or_default()
            .push(config.clone());
        config
    }

    /// Retrieve a specific push notification config.
    pub fn get_push_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Option<PushNotificationConfig> {
        let configs = self.read_push_configs();
        configs
            .get(task_id)?
            .iter()
            .find(|c| c.id == config_id)
            .cloned()
    }

    /// List all push notification configs for a task.
    pub fn list_push_configs(&self, task_id: &str) -> Vec<PushNotificationConfig> {
        let configs = self.read_push_configs();
        configs.get(task_id).cloned().unwrap_or_default()
    }

    /// Remove a push notification config.
    pub fn delete_push_config(&self, task_id: &str, config_id: &str) {
        let mut configs = self.write_push_configs();
        if let Some(entry) = configs.get_mut(task_id) {
            entry.retain(|c| c.id != config_id);
        }
    }

    /// Send push notifications to all registered webhooks for a task.
    ///
    /// The payload uses the StreamResponse format (spec §4.3.3), matching the
    /// SSE handlers: `{"statusUpdate": {...}}` or `{"artifactUpdate": {...}}`.
    fn notify_push_configs(&self, _task_id: &str, event: &TaskEvent) {
        let configs = self.read_push_configs();
        let task_id = match event {
            TaskEvent::StatusChanged { task_id, .. } => task_id,
            TaskEvent::ArtifactAdded { task_id, .. } => task_id,
        };
        if let Some(entries) = configs.get(task_id) {
            let context_id = self
                .read_tasks()
                .get(task_id)
                .and_then(|t| t.context_id.clone());
            let payload = task_event_to_stream_response(event, &context_id);
            let Some(transport) = self.transport.clone() else {
                log::warn!("push notification skipped: no transport configured");
                return;
            };
            for config in entries {
                let config = config.clone();
                let payload = payload.clone();
                let transport = transport.clone();
                tokio::spawn(async move {
                    // Nested spawn so panics are caught via JoinHandle error
                    if let Err(e) = tokio::spawn(async move {
                        deliver_push(&transport, config, payload).await;
                    })
                    .await
                    {
                        log::warn!("push notification delivery panicked: {e}");
                    }
                });
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task CRUD
    // -----------------------------------------------------------------------

    /// Create a new task in `Submitted` state.
    pub fn create_task(
        &self,
        context_id: Option<String>,
        parent_task_id: Option<String>,
        metadata: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Task {
        let id = Uuid::new_v4().to_string();
        let task = Task {
            id: id.clone(),
            context_id,
            parent_task_id,
            status: TaskStatus {
                state: TaskState::Submitted,
                timestamp: Utc::now(),
                message: None,
            },
            history: None,
            artifacts: vec![],
            created_at: Some(Utc::now()),
            metadata,
        };
        let mut tasks = self.write_tasks();
        tasks.insert(id.clone(), task.clone());
        self.assign_update_seq(&id);
        task
    }

    /// Create a task with an idempotency key (A2A spec §3.3.1).
    ///
    /// If the key already exists, returns `Err((existing_task, true))`.
    /// If the key is new, creates a task, stores the mapping, and returns
    /// `Ok(task)`.
    pub fn create_task_with_idempotency(
        &self,
        key: &str,
        context_id: Option<String>,
        parent_task_id: Option<String>,
        metadata: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<Task, Box<(Task, bool)>> {
        // Single write lock for the entire check-and-insert to prevent TOCTOU race.
        let mut keys = self.write_idempotency_keys();
        if let Some(existing_id) = keys.get(key)
            && let Ok(task) = self.get_task(existing_id)
        {
            return Err(Box::new((task, true))); // True = was duplicate
        }

        // Create new task
        let task = self.create_task(context_id, parent_task_id, metadata);
        keys.insert(key.to_string(), task.id.clone());
        Ok(task)
    }

    /// Retrieve a task by ID.
    pub fn get_task(&self, id: &str) -> Result<Task, A2aError> {
        self.read_tasks()
            .get(id)
            .cloned()
            .ok_or_else(|| A2aError::TaskNotFound(id.to_string()))
    }

    /// Transition a task to a new state (with an optional message).
    ///
    /// Returns `InvalidStateTransition` if the transition is not allowed by
    /// the A2A task state machine.
    pub fn update_status(
        &self,
        id: &str,
        new_state: TaskState,
        message: Option<Message>,
    ) -> Result<Task, A2aError> {
        let mut tasks = self.write_tasks();
        let task = tasks
            .get_mut(id)
            .ok_or_else(|| A2aError::TaskNotFound(id.to_string()))?;

        if !is_valid_transition(&task.status.state, &new_state) {
            return Err(A2aError::InvalidStateTransition {
                from: task.status.state.clone(),
                to: new_state.clone(),
            });
        }

        let new_status = TaskStatus {
            state: new_state,
            timestamp: Utc::now(),
            message,
        };
        task.status = new_status.clone();
        let result = task.clone();
        self.assign_update_seq(id);
        drop(tasks); // release lock before notifying
        self.notify_status_change(id, &new_status);
        Ok(result)
    }

    /// Append an artifact to a task.
    pub fn add_artifact(&self, id: &str, artifact: Artifact) -> Result<Task, A2aError> {
        let mut tasks = self.write_tasks();
        let task = tasks
            .get_mut(id)
            .ok_or_else(|| A2aError::TaskNotFound(id.to_string()))?;
        task.artifacts.push(artifact.clone());
        let result = task.clone();
        drop(tasks); // release lock before notifying
        self.notify_artifact_added(id, &artifact);
        Ok(result)
    }

    /// List all tasks, optionally filtered by state.
    pub fn list_tasks(&self, filter: Option<TaskState>) -> Vec<Task> {
        let tasks = self.read_tasks();
        match filter {
            Some(state) => tasks
                .values()
                .filter(|t| t.status.state == state)
                .cloned()
                .collect(),
            None => tasks.values().cloned().collect(),
        }
    }

    /// List tasks with optional status/context filtering and cursor-based pagination.
    ///
    /// Returns a tuple of `(tasks, next_page_token)` where `next_page_token` is
    /// `Some(task_id)` if there are more results beyond the requested `limit`.
    pub fn list_tasks_filtered(
        &self,
        status: Option<TaskState>,
        context_id: Option<&str>,
        limit: usize,
        cursor: Option<&str>,
    ) -> (Vec<Task>, Option<String>) {
        let tasks = self.read_tasks();
        let mut filtered: Vec<Task> = tasks.values().cloned().collect();

        // Filter by status if provided
        if let Some(state) = &status {
            filtered.retain(|t| t.status.state == *state);
        }

        // Filter by context ID if provided (spec §11.5)
        if let Some(ctx) = context_id {
            filtered.retain(|t| t.context_id.as_deref() == Some(ctx));
        }

        // Sort by status timestamp descending (spec §3.1.4), then by the
        // store-assigned update sequence descending, then by task ID ascending
        // so equal timestamps still order deterministically (HashMap iteration
        // order is not stable).
        let seq = self.read_update_seq();
        filtered.sort_by(|a, b| {
            b.status
                .timestamp
                .cmp(&a.status.timestamp)
                .then_with(|| seq.get(&b.id).cmp(&seq.get(&a.id)))
                .then_with(|| a.id.cmp(&b.id))
        });

        // Apply cursor-based pagination
        if let Some(cursor_id) = cursor
            && let Some(pos) = filtered.iter().position(|t| t.id == cursor_id)
        {
            filtered = filtered.split_off(pos + 1);
        }

        // Apply limit and determine if there are more results
        let has_more = filtered.len() > limit;
        filtered.truncate(limit);

        let next_token = if has_more {
            filtered.last().map(|t| t.id.clone())
        } else {
            None
        };

        (filtered, next_token)
    }

    /// Cancel a task (delegates to `update_status`).
    pub fn cancel_task(&self, id: &str) -> Result<Task, A2aError> {
        self.update_status(id, TaskState::Canceled, None)
    }

    /// Fail a task with a reason (delegates to `update_status`).
    pub fn fail_task(&self, id: &str, reason: &str) -> Result<Task, A2aError> {
        self.update_status(id, TaskState::Failed, Some(agent_message(reason)))
    }

    /// Reject a task with a reason (delegates to `update_status`).
    pub fn reject_task(&self, id: &str, reason: &str) -> Result<Task, A2aError> {
        self.update_status(id, TaskState::Rejected, Some(agent_message(reason)))
    }

    /// Complete a task with a result artifact.
    ///
    /// Transitions the task to `Completed` and appends the result as a
    /// `Part::Text` artifact named `"result"`.
    pub fn complete_task(&self, id: &str, result: &str) -> Result<Task, A2aError> {
        let mut tasks = self.write_tasks();
        let task = tasks
            .get_mut(id)
            .ok_or_else(|| A2aError::TaskNotFound(id.to_string()))?;

        if !is_valid_transition(&task.status.state, &TaskState::Completed) {
            return Err(A2aError::InvalidStateTransition {
                from: task.status.state.clone(),
                to: TaskState::Completed,
            });
        }

        let result_artifact = Artifact {
            artifact_id: Uuid::new_v4().to_string(),
            name: Some("result".to_string()),
            parts: vec![Part::Text {
                text: result.to_string(),
            }],
            metadata: None,
        };
        task.artifacts.push(result_artifact.clone());

        task.status = TaskStatus {
            state: TaskState::Completed,
            timestamp: Utc::now(),
            message: Some(Message {
                role: Role::Agent,
                parts: vec![Part::Text {
                    text: "Task completed successfully".to_string(),
                }],
                message_id: Uuid::new_v4().to_string(),
                extensions: None,
                metadata: None,
            }),
        };

        let status = task.status.clone();
        let completed_task = task.clone();
        let id_owned = id.to_string();
        drop(tasks); // release lock before notifying
        self.notify_artifact_added(&id_owned, &result_artifact);
        self.notify_status_change(&id_owned, &status);

        Ok(completed_task)
    }

    /// Append a message to a task's history.
    ///
    /// If the task has no history yet, a new vector is created.
    pub fn append_history(&self, id: &str, message: Message) -> Result<Task, A2aError> {
        let mut tasks = self.write_tasks();
        let task = tasks
            .get_mut(id)
            .ok_or_else(|| A2aError::TaskNotFound(id.to_string()))?;

        if matches!(
            task.status.state,
            TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
        ) {
            return Err(A2aError::InvalidStateTransition {
                from: task.status.state.clone(),
                to: task.status.state.clone(),
            });
        }

        task.history.get_or_insert_with(Vec::new).push(message);
        Ok(task.clone())
    }

    // -----------------------------------------------------------------------
    // Update sequence
    // -----------------------------------------------------------------------

    /// Assign the next monotonic update sequence to `id`.
    ///
    /// Callers hold the `tasks` write lock, so a list call never observes a
    /// task with a new status but a stale sequence.
    fn assign_update_seq(&self, id: &str) {
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        self.write_update_seq().insert(id.to_string(), seq);
    }
}

/// Convert an internal [`TaskEvent`] into the A2A StreamResponse JSON format
/// (spec §4.2.1, §4.2.2, §4.3.3).
///
/// `TaskEvent::StatusChanged` becomes `{"statusUpdate": {...}}` and
/// `TaskEvent::ArtifactAdded` becomes `{"artifactUpdate": {...}}` — the same
/// shape the SSE handlers emit. `context_id` is included when the task has one.
pub fn task_event_to_stream_response(event: &TaskEvent, context_id: &Option<String>) -> Value {
    match event {
        TaskEvent::StatusChanged { task_id, status } => {
            let mut update = serde_json::json!({
                "taskId": task_id,
                "status": status,
            });
            if let Some(ctx) = context_id {
                update["contextId"] = serde_json::json!(ctx);
            }
            serde_json::json!({ "statusUpdate": update })
        }
        TaskEvent::ArtifactAdded { task_id, artifact } => {
            let mut update = serde_json::json!({
                "taskId": task_id,
                "artifact": artifact,
            });
            if let Some(ctx) = context_id {
                update["contextId"] = serde_json::json!(ctx);
            }
            serde_json::json!({ "artifactUpdate": update })
        }
    }
}

/// Deliver a push notification to a single webhook endpoint.
///
/// Applies any configured authentication scheme (Bearer, Basic, or Custom).
/// Custom schemes are not supported for direct webhook delivery and will log
/// a warning.
async fn deliver_push(transport: &reqwest::Client, config: PushNotificationConfig, payload: Value) {
    let mut req = transport
        .post(&config.url)
        .header("a2a-version", crate::A2A_VERSION)
        .json(&payload);

    if let Some(auth) = &config.authentication {
        match &auth.scheme {
            PushAuthScheme::Bearer { token } => {
                req = req.header("Authorization", format!("Bearer {token}"));
            }
            PushAuthScheme::Basic { username, password } => {
                req = req.basic_auth(username, Some(password));
            }
            PushAuthScheme::Custom { .. } => {
                log::warn!("custom push auth scheme not supported for webhook delivery");
            }
        }
    }

    if let Err(e) = req.send().await {
        log::warn!("push notification to {} failed: {e}", config.url);
    }
}

/// Validate a state transition in the A2A task state machine.
///
/// Valid transitions:
/// - `Submitted` → `Working` | `Canceled` | `Rejected`
/// - `Working` → `InputRequired` | `Completed` | `Failed` | `Canceled` | `Rejected`
/// - `InputRequired` → `Working` | `Canceled`
///
/// All transitions from terminal states (`Completed`, `Failed`, `Canceled`,
/// `Rejected`) are invalid.
pub fn is_valid_transition(from: &TaskState, to: &TaskState) -> bool {
    matches!(
        (from, to),
        (TaskState::Submitted, TaskState::Working)
            | (TaskState::Submitted, TaskState::Canceled)
            | (TaskState::Submitted, TaskState::Rejected)
            | (TaskState::Working, TaskState::InputRequired)
            | (TaskState::Working, TaskState::Completed)
            | (TaskState::Working, TaskState::Failed)
            | (TaskState::Working, TaskState::Canceled)
            | (TaskState::Working, TaskState::Rejected)
            | (TaskState::InputRequired, TaskState::Working)
            | (TaskState::InputRequired, TaskState::Canceled)
    )
}

/// Build an agent-authored status message carrying `text`.
fn agent_message(text: &str) -> Message {
    Message {
        role: Role::Agent,
        parts: vec![Part::Text {
            text: text.to_string(),
        }],
        message_id: Uuid::new_v4().to_string(),
        extensions: None,
        metadata: None,
    }
}
