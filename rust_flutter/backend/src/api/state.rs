use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::orchestrator::engine::OrchestratorEngine;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiTotals {
    pub prompt_tokens: u64,
    pub candidate_tokens: u64,
    pub total_requests: u64,
    pub total_runtime_seconds: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub orchestrator: Arc<Mutex<OrchestratorEngine>>,
    pub gemini_totals: Arc<Mutex<GeminiTotals>>,
    pub refresh_tx: mpsc::Sender<()>,
}
