use axum::{routing::get, Json, Router};
use serde::Serialize;

#[derive(Serialize)]
pub struct BuildMetadata {
    pub version: &'static str,
    pub git_sha: Option<&'static str>,
    pub built_at: Option<&'static str>,
}

#[derive(Serialize)]
pub struct Health {
    pub status: &'static str,
    pub build: BuildMetadata,
}

pub async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        build: BuildMetadata {
            version: env!("CARGO_PKG_VERSION"),
            git_sha: option_env!("APP_BUILD_SHA"),
            built_at: option_env!("APP_BUILD_TIMESTAMP"),
        },
    })
}

pub fn routes() -> Router {
    Router::new().route("/api/v1/health", get(health))
}
