use axum::{
    extract::{State, Path},
    response::IntoResponse,
    Json,
};
use serde_json::json;

use crate::api::state::AppState;

pub async fn get_state(State(state): State<AppState>) -> impl IntoResponse {
    let engine = state.orchestrator.lock().await;
    let totals = state.gemini_totals.lock().await;

    let running_sessions: Vec<_> = engine.running.iter().map(|(id, session)| {
        json!({
            "issue_id": id,
            "started_at": session.started_at,
            "last_heartbeat": session.last_heartbeat,
        })
    }).collect();

    let retry_queue: Vec<_> = engine.retry_attempts.iter().map(|(id, retry)| {
        json!({
            "issue_id": id,
            "attempt_count": retry.attempt_count,
            "next_retry_at": retry.next_retry_at,
        })
    }).collect();

    let response = json!({
        "counts": {
            "running": engine.running.len(),
            "claimed": engine.claimed.len(),
            "retries": engine.retry_attempts.len(),
        },
        "running_sessions": running_sessions,
        "retry_queue": retry_queue,
        "gemini_totals": {
            "prompt_tokens": totals.prompt_tokens,
            "candidate_tokens": totals.candidate_tokens,
            "total_requests": totals.total_requests,
            "total_runtime_seconds": totals.total_runtime_seconds,
        }
    });

    Json(response)
}

pub async fn get_issue_status(
    Path(issue_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let engine = state.orchestrator.lock().await;

    let is_running = engine.running.contains_key(&issue_id);
    let is_claimed = engine.claimed.contains(&issue_id);
    let retry_info = engine.retry_attempts.get(&issue_id).map(|r| {
        json!({
            "attempt_count": r.attempt_count,
            "next_retry_at": r.next_retry_at,
        })
    });

    let session_info = engine.running.get(&issue_id).map(|s| {
        json!({
            "started_at": s.started_at,
            "last_heartbeat": s.last_heartbeat,
        })
    });

    let response = json!({
        "issue_id": issue_id,
        "status": {
            "is_running": is_running,
            "is_claimed": is_claimed,
            "retry_info": retry_info,
            "session_info": session_info,
            "logs": "Detailed logs not currently tracked in OrchestratorEngine state.",
        }
    });

    Json(response)
}

pub async fn trigger_refresh(State(state): State<AppState>) -> impl IntoResponse {
    match state.refresh_tx.try_send(()) {
        Ok(_) => Json(json!({ "status": "refresh triggered" })),
        Err(_) => Json(json!({ "status": "refresh already pending or channel full" })),
    }
}
