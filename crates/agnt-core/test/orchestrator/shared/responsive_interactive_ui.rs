use super::*;

pub(crate) struct ResponsiveInteractiveUi {
    pub(crate) event_tx: mpsc::Sender<OrchestratorEvent>,
    pub(crate) submitted: std::collections::VecDeque<String>,
    pub(crate) model_switch_requests: std::collections::VecDeque<String>,
    pub(crate) quit: Arc<AtomicBool>,
    pub(crate) active: Arc<AtomicBool>,
    pub(crate) completed_count: Arc<AtomicUsize>,
    pub(crate) expected_completions: usize,
    pub(crate) active_pump_count: Arc<AtomicUsize>,
    pub(crate) warnings: Arc<Mutex<Vec<String>>>,
    pub(crate) drained_tx: Arc<tokio::sync::watch::Sender<bool>>,
    pub(crate) _bus_task: Option<tokio::task::JoinHandle<()>>,
}

impl ResponsiveInteractiveUi {
    pub(crate) fn new(
        initial_prompts: &[&str],
        injected_during_active: &[&str],
        injected_model_switch_during_active: &[&str],
        active: Arc<AtomicBool>,
        _block_first_turn: Arc<AtomicBool>,
        expected_completions: usize,
        active_pump_count: Arc<AtomicUsize>,
    ) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        let (drained_tx, _) = tokio::sync::watch::channel(false);
        let mut submitted: std::collections::VecDeque<String> =
            initial_prompts.iter().map(|s| s.to_string()).collect();
        submitted.extend(injected_during_active.iter().map(|s| s.to_string()));
        let model_switch_requests: std::collections::VecDeque<String> =
            injected_model_switch_during_active
                .iter()
                .map(|s| s.to_string())
                .collect();
        Self {
            event_tx,
            submitted,
            model_switch_requests,
            quit: Arc::new(AtomicBool::new(false)),
            active,
            completed_count: Arc::new(AtomicUsize::new(0)),
            expected_completions,
            active_pump_count,
            warnings: Arc::new(Mutex::new(Vec::new())),
            drained_tx: Arc::new(drained_tx),
            _bus_task: None,
        }
    }

    /// Signals when the spawner has injected every queued prompt and model
    /// switch while the worker was active. Tests await this before unblocking
    /// the turn, so the injections settle without a fixed sleep.
    pub(crate) fn drained_rx(&self) -> tokio::sync::watch::Receiver<bool> {
        self.drained_tx.subscribe()
    }

    pub(crate) fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let quit = Arc::clone(&self.quit);
        let completed_count = Arc::clone(&self.completed_count);
        let active = Arc::clone(&self.active);
        let active_pump_count = Arc::clone(&self.active_pump_count);
        let warnings = Arc::clone(&self.warnings);
        let expected_completions = self.expected_completions;

        let mut ui_event_rx = bus.ui_event().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = ui_event_rx.recv() => {
                        match event {
                            SystemEvent::TurnCompleted { .. } => {
                                if active.load(Ordering::SeqCst) {
                                    active_pump_count.fetch_add(1, Ordering::SeqCst);
                                }
                                let count = completed_count.fetch_add(1, Ordering::SeqCst) + 1;
                                if count >= expected_completions {
                                    quit.store(true, Ordering::SeqCst);
                                }
                            }
                            SystemEvent::SystemMessage { text, .. } => {
                                warnings.lock().expect("warnings lock").push(text);
                            }
                            _ => {}
                        }
                    }
                    else => break,
                }
            }
        }));
        self
    }

    pub(crate) fn make_event_spawner(
        &self,
    ) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let submitted = self.submitted.clone();
        let model_switch_requests = self.model_switch_requests.clone();
        let active = Arc::clone(&self.active);
        let quit = Arc::clone(&self.quit);
        let drained_tx = Arc::clone(&self.drained_tx);
        move |event_tx| {
            let event_tx = event_tx.clone();
            let mut submitted = submitted.clone();
            let mut model_switch_requests = model_switch_requests.clone();
            tokio::spawn(async move {
                // Send the first (initial) prompt immediately, then gate any
                // injected-during-active prompts and model switches on the
                // worker being active.
                if let Some(prompt) = submitted.pop_front() {
                    let _ = event_tx
                        .send(OrchestratorEvent::PromptSubmitted { text: prompt })
                        .await;
                }
                loop {
                    if active.load(Ordering::SeqCst) {
                        if let Some(prompt) = submitted.pop_front() {
                            let _ = event_tx
                                .send(OrchestratorEvent::PromptSubmitted { text: prompt })
                                .await;
                        }
                        if let Some(spec) = model_switch_requests.pop_front() {
                            let _ = event_tx
                                .send(OrchestratorEvent::UiRequest(UiRequest::SwitchModel {
                                    spec,
                                }))
                                .await;
                        }
                        if submitted.is_empty() && model_switch_requests.is_empty() {
                            let _ = drained_tx.send(true);
                        }
                    }
                    if quit.load(Ordering::SeqCst)
                        && event_tx.send(OrchestratorEvent::Quit).await.is_err()
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
        }
    }
}

impl UserInputUi for ResponsiveInteractiveUi {
    fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}
