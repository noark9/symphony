//! HTTP server for the observability dashboard and REST API.
//!
//! Implements SPEC §13.7.

use crate::orchestrator::SharedState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde_json::json;
use std::path::PathBuf;
use tracing::info;

/// Application state shared with handlers.
#[derive(Clone)]
pub struct AppState {
    pub orchestrator: SharedState,
    pub workflow_path: PathBuf,
}

/// Start the HTTP server (§13.7).
pub async fn start_server(
    port: u16,
    orchestrator_state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app_state = AppState {
        orchestrator: orchestrator_state,
        workflow_path: PathBuf::from("WORKFLOW.md"),
    };

    // Try to serve the built React frontend from frontend/dist
    let _frontend_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("frontend")
        .join("dist");

    let app = Router::new()
        .route("/api/v1/state", get(get_state))
        .route("/api/v1/refresh", post(post_refresh))
        .route("/api/v1/{issue_identifier}", get(get_issue))
        .route("/", get(dashboard_handler))
        .with_state(app_state);

    let addr = format!("127.0.0.1:{}", port);
    info!(addr = %addr, "starting HTTP server");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

/// GET /api/v1/state — system state snapshot (§13.7.2).
async fn get_state(State(state): State<AppState>) -> impl IntoResponse {
    let s = state.orchestrator.read().await;
    Json(s.snapshot())
}

/// GET /api/v1/:issue_identifier — issue-specific details (§13.7.2).
async fn get_issue(
    State(state): State<AppState>,
    Path(issue_identifier): Path<String>,
) -> impl IntoResponse {
    let s = state.orchestrator.read().await;

    match s.issue_detail(&issue_identifier) {
        Some(detail) => Json(json!(detail)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "code": "issue_not_found",
                    "message": format!("Issue {} not found in current state", issue_identifier)
                }
            })),
        )
            .into_response(),
    }
}

/// POST /api/v1/refresh — trigger immediate poll (§13.7.2).
async fn post_refresh(State(state): State<AppState>) -> impl IntoResponse {
    let workflow_path = state.workflow_path.clone();
    crate::orchestrator::trigger_refresh(state.orchestrator, workflow_path).await;

    (
        StatusCode::ACCEPTED,
        Json(json!({
            "queued": true,
            "coalesced": false,
            "requested_at": chrono::Utc::now().to_rfc3339(),
            "operations": ["poll", "reconcile"]
        })),
    )
}

/// Dashboard handler — serves basic HTML when React build is not available.
async fn dashboard_handler() -> impl IntoResponse {
    Html(fallback_dashboard_html())
}

/// Fallback dashboard HTML when React SPA is not built.
fn fallback_dashboard_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Symphony Dashboard</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #0d1117;
            color: #c9d1d9;
            padding: 24px;
        }
        h1 { color: #58a6ff; margin-bottom: 8px; }
        .subtitle { color: #8b949e; margin-bottom: 24px; }
        .card {
            background: #161b22;
            border: 1px solid #30363d;
            border-radius: 8px;
            padding: 16px;
            margin-bottom: 16px;
        }
        .card h2 { color: #58a6ff; font-size: 14px; margin-bottom: 12px; text-transform: uppercase; letter-spacing: 1px; }
        .stat { display: inline-block; margin-right: 24px; }
        .stat-value { font-size: 24px; font-weight: bold; color: #f0f6fc; }
        .stat-label { font-size: 12px; color: #8b949e; }
        table { width: 100%; border-collapse: collapse; }
        th { text-align: left; color: #8b949e; font-size: 12px; padding: 8px; border-bottom: 1px solid #30363d; }
        td { padding: 8px; border-bottom: 1px solid #21262d; font-size: 13px; }
        .status-running { color: #3fb950; }
        .status-retrying { color: #d29922; }
        .refresh-btn {
            background: #238636;
            color: white;
            border: none;
            padding: 8px 16px;
            border-radius: 6px;
            cursor: pointer;
            font-size: 13px;
        }
        .refresh-btn:hover { background: #2ea043; }
        #error { color: #f85149; display: none; margin-bottom: 16px; }
        #last-update { color: #8b949e; font-size: 12px; }
        details { cursor: pointer; }
        details summary { color: #d29922; font-size: 12px; user-select: none; }
        details summary:hover { color: #e3b341; }
        details pre {
            margin-top: 8px;
            padding: 8px;
            background: #0d1117;
            border: 1px solid #30363d;
            border-radius: 4px;
            font-size: 11px;
            color: #f85149;
            white-space: pre-wrap;
            word-break: break-all;
            max-height: 300px;
            overflow-y: auto;
        }
    </style>
</head>
<body>
    <h1>⚡ Symphony</h1>
    <p class="subtitle">Automation Service Dashboard</p>
    <div id="error"></div>

    <div class="card">
        <h2>Overview</h2>
        <div id="overview">Loading...</div>
    </div>

    <div class="card">
        <h2>Running Sessions</h2>
        <div id="running">No running sessions</div>
    </div>

    <div class="card">
        <h2>Retry Queue</h2>
        <div id="retrying">No retries pending</div>
    </div>

    <div class="card">
        <h2>Token Totals</h2>
        <div id="totals">-</div>
    </div>

    <div style="margin-top: 16px; display: flex; align-items: center; gap: 16px;">
        <button class="refresh-btn" onclick="refresh()">⟳ Refresh Now</button>
        <span id="last-update"></span>
    </div>

    <script>
        async function fetchState() {
            try {
                const res = await fetch('/api/v1/state');
                const data = await res.json();
                renderState(data);
                document.getElementById('error').style.display = 'none';
            } catch (e) {
                document.getElementById('error').textContent = 'Failed to fetch state: ' + e.message;
                document.getElementById('error').style.display = 'block';
            }
        }

        function escapeHtml(str) {
            if (!str) return '';
            return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
        }

        function renderErrorCell(error) {
            if (!error) return '-';
            const firstLine = error.split('\n')[0];
            const preview = firstLine.length > 60 ? firstLine.substring(0, 60) + '…' : firstLine;
            if (error.includes('\n') || error.length > 80) {
                return '<details><summary>' + escapeHtml(preview) + ' ▸</summary><pre>' + escapeHtml(error) + '</pre></details>';
            }
            return '<span style="color:#f85149">' + escapeHtml(error) + '</span>';
        }

        function renderState(data) {
            document.getElementById('overview').innerHTML = `
                <div class="stat"><div class="stat-value">${data.counts.running}</div><div class="stat-label">Running</div></div>
                <div class="stat"><div class="stat-value">${data.counts.retrying}</div><div class="stat-label">Retrying</div></div>
            `;

            if (data.running.length > 0) {
                let html = '<table><tr><th>Issue</th><th>State</th><th>Turn</th><th>Last Event</th><th>Last Message</th><th>Tokens</th><th>Started</th></tr>';
                for (const r of data.running) {
                    const msgCell = r.last_message ? renderErrorCell(r.last_message) : '-';
                    html += `<tr>
                        <td><strong>${escapeHtml(r.issue_identifier)}</strong></td>
                        <td class="status-running">${escapeHtml(r.state)}</td>
                        <td>${r.turn_count}</td>
                        <td>${escapeHtml(r.last_event) || '-'}</td>
                        <td>${msgCell}</td>
                        <td>${(r.tokens.total_tokens || 0).toLocaleString()}</td>
                        <td>${new Date(r.started_at).toLocaleTimeString()}</td>
                    </tr>`;
                }
                html += '</table>';
                document.getElementById('running').innerHTML = html;
            } else {
                document.getElementById('running').innerHTML = 'No running sessions';
            }

            if (data.retrying.length > 0) {
                let html = '<table><tr><th>Issue</th><th>Attempt</th><th>Due At</th><th>Error</th></tr>';
                for (const r of data.retrying) {
                    html += `<tr>
                        <td><strong>${escapeHtml(r.issue_identifier)}</strong></td>
                        <td class="status-retrying">${r.attempt}</td>
                        <td>${new Date(r.due_at).toLocaleTimeString()}</td>
                        <td>${renderErrorCell(r.error)}</td>
                    </tr>`;
                }
                html += '</table>';
                document.getElementById('retrying').innerHTML = html;
            } else {
                document.getElementById('retrying').innerHTML = 'No retries pending';
            }

            const t = data.gemini_totals;
            document.getElementById('totals').innerHTML = `
                <div class="stat"><div class="stat-value">${t.input_tokens.toLocaleString()}</div><div class="stat-label">Input Tokens</div></div>
                <div class="stat"><div class="stat-value">${t.output_tokens.toLocaleString()}</div><div class="stat-label">Output Tokens</div></div>
                <div class="stat"><div class="stat-value">${t.total_tokens.toLocaleString()}</div><div class="stat-label">Total Tokens</div></div>
                <div class="stat"><div class="stat-value">${Math.round(t.seconds_running)}s</div><div class="stat-label">Runtime</div></div>
            `;

            document.getElementById('last-update').textContent = 'Last updated: ' + new Date().toLocaleTimeString();
        }

        async function refresh() {
            await fetch('/api/v1/refresh', { method: 'POST' });
            setTimeout(fetchState, 500);
        }

        fetchState();
        setInterval(fetchState, 2000);
    </script>
</body>
</html>"#
        .to_string()
}
