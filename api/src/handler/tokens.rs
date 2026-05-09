use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, patch, post},
    Json, Router,
};
use std::sync::Arc;

use crate::{
    error::AppError,
    model::{TokenSecret, TokenSummary},
    service::tokens::{CreateTokenRequest, ReplaceClaimsRequest},
    state::AppState,
};

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/v1/tokens", get(list_tokens).post(create_token))
        .route("/api/v1/tokens/:id", get(get_token))
        .route("/api/v1/tokens/:id/rotate", post(rotate_token))
        .route("/api/v1/tokens/:id/revoke", post(revoke_token))
        .route("/api/v1/tokens/:id/claims", patch(replace_claims))
        .with_state(state)
}

async fn create_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateTokenRequest>,
) -> Result<Json<TokenSecret>, AppError> {
    state.auth().require_admin(&headers).await?;
    Ok(Json(state.tokens().create(payload).await?))
}

async fn list_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<TokenSummary>>, AppError> {
    state.auth().require_admin(&headers).await?;
    Ok(Json(state.tokens().list().await?))
}

async fn get_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TokenSummary>, AppError> {
    state.auth().require_admin(&headers).await?;
    Ok(Json(state.tokens().find(&id).await?))
}

async fn rotate_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TokenSecret>, AppError> {
    state.auth().require_admin(&headers).await?;
    Ok(Json(state.tokens().rotate(&id).await?))
}

async fn revoke_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TokenSummary>, AppError> {
    state.auth().require_admin(&headers).await?;
    Ok(Json(state.tokens().revoke(&id).await?))
}

async fn replace_claims(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<ReplaceClaimsRequest>,
) -> Result<Json<TokenSummary>, AppError> {
    state.auth().require_admin(&headers).await?;
    Ok(Json(state.tokens().replace_claims(&id, payload).await?))
}
