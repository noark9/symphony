use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};
use crate::api::state::AppState;
use crate::api::routes::{get_state, get_issue_status, trigger_refresh};

pub fn app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/v1/state", get(get_state))
        .route("/api/v1/refresh", post(trigger_refresh))
        .route("/api/v1/{issue_identifier}", get(get_issue_status))
        .layer(cors)
        .with_state(state)
}
