use axum::{
    body::{to_bytes, Body},
    extract::{Query, State},
    http::{HeaderMap, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::{error::AppError, service::crate_name::crate_name_from_sparse_path, state::AppState};

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/-/ping", get(ping))
        .route("/-/whoami", get(whoami))
        .route("/api/v1/crates/config.json", get(config))
        .route("/api/v1/crates", get(search))
        .route("/api/v1/crates/new", put(publish))
        .fallback(cargo_fallback)
        .with_state(state)
}

async fn ping() -> Json<Value> {
    Json(serde_json::json!({ "ok": true }))
}

async fn whoami(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let principal = state.auth().authenticate(&headers).await?;
    Ok(Json(serde_json::json!({
        "username": principal.token_id,
        "bootstrap": principal.bootstrap
    })))
}

async fn config(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.registry().sparse_config();
    Json(serde_json::json!({
        "dl": config.dl,
        "api": config.api,
        "auth-required": config.auth_required
    }))
}

async fn search(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<SearchParams>,
) -> Result<Json<Value>, AppError> {
    let principal = state.auth().authenticate(&headers).await?;
    state.auth().require_read(&principal, "*")?;
    Ok(Json(
        state
            .registry()
            .search(
                params.q.as_deref().unwrap_or_default(),
                params.per_page.unwrap_or(10),
            )
            .await?,
    ))
}

async fn publish(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    req: Request<Body>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let principal = state.auth().authenticate(&headers).await?;
    let limit = state.config().max_tarball_bytes() + 1024 * 1024;
    let body = to_bytes(req.into_body(), limit)
        .await
        .map_err(|_| AppError::BadRequest("failed to read publish body".to_owned()))?;
    let metadata_name = crate_name_from_publish_body(&body)?;
    state.auth().require_publish(&principal, &metadata_name)?;
    Ok((
        StatusCode::OK,
        Json(state.registry().publish(&principal, &body).await?),
    ))
}

async fn cargo_fallback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    req: Request<Body>,
) -> Response {
    match route_cargo(state, headers, req).await {
        Ok(response) => response,
        Err(err) => err.into_response(),
    }
}

async fn route_cargo(
    state: Arc<AppState>,
    headers: HeaderMap,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let method = req.method().clone();
    let path = req
        .uri()
        .path()
        .strip_prefix("/api/v1/crates/")
        .ok_or(AppError::NotFound)?
        .to_owned();

    if let Some((name, version, action)) = parse_crate_action_path(&path) {
        match (method, action) {
            (Method::GET, "download") => return download(state, headers, name, version).await,
            (Method::DELETE, "yank") => return yank(state, headers, name, version).await,
            (Method::PUT, "unyank") => return unyank(state, headers, name, version).await,
            _ => return Err(AppError::NotFound),
        }
    }

    if method != Method::GET {
        return Err(AppError::NotFound);
    }
    sparse_index(state, headers, path).await
}

async fn sparse_index(
    state: Arc<AppState>,
    headers: HeaderMap,
    path: String,
) -> Result<Response, AppError> {
    let crate_name = crate_name_from_sparse_path(&path)?;
    let principal = state.auth().authenticate(&headers).await?;
    state.auth().require_read(&principal, &crate_name)?;
    let body = state.registry().sparse_index(&crate_name).await?;
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        body,
    )
        .into_response())
}

async fn download(
    state: Arc<AppState>,
    headers: HeaderMap,
    name: String,
    version: String,
) -> Result<Response, AppError> {
    let principal = state.auth().authenticate(&headers).await?;
    state.auth().require_read(&principal, &name)?;
    let download = state.registry().download(&name, &version).await?;
    Ok((download.headers, download.bytes).into_response())
}

async fn yank(
    state: Arc<AppState>,
    headers: HeaderMap,
    name: String,
    version: String,
) -> Result<Response, AppError> {
    let principal = state.auth().authenticate(&headers).await?;
    state.auth().require_publish(&principal, &name)?;
    Ok(Json(state.registry().yank(&name, &version, true).await?).into_response())
}

async fn unyank(
    state: Arc<AppState>,
    headers: HeaderMap,
    name: String,
    version: String,
) -> Result<Response, AppError> {
    let principal = state.auth().authenticate(&headers).await?;
    state.auth().require_publish(&principal, &name)?;
    Ok(Json(state.registry().yank(&name, &version, false).await?).into_response())
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    per_page: Option<u64>,
}

fn crate_name_from_publish_body(body: &[u8]) -> Result<String, AppError> {
    let length_bytes = body
        .get(0..4)
        .ok_or_else(|| AppError::BadRequest("publish body too short".to_owned()))?;
    let metadata_len = u32::from_le_bytes(length_bytes.try_into().expect("slice length checked"));
    let metadata_bytes = body
        .get(4..4 + metadata_len as usize)
        .ok_or_else(|| AppError::BadRequest("publish metadata length mismatch".to_owned()))?;
    let metadata: Value = serde_json::from_slice(metadata_bytes)
        .map_err(|_| AppError::BadRequest("invalid publish metadata JSON".to_owned()))?;
    metadata
        .get("name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| AppError::BadRequest("publish metadata missing name".to_owned()))
}

fn parse_crate_action_path(path: &str) -> Option<(String, String, &str)> {
    let mut parts = path.split('/');
    let name = parts.next()?.to_owned();
    let version = parts.next()?.to_owned();
    let action = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if !matches!(action, "download" | "yank" | "unyank") {
        return None;
    }
    Some((name, version, action))
}
