use axum::Router;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};

mod config;
mod db;
mod error;
mod handler;
mod model;
mod repo;
mod schema;
mod service;
mod state;
mod telemetry;

use state::AppState;

#[tokio::main]
async fn main() {
    telemetry::init_tracing("mini-crates-api");

    let state = AppState::new().await;
    let app = Router::new()
        .merge(handler::health::routes())
        .merge(handler::tokens::routes(state.clone()))
        .merge(handler::cargo::routes(state.clone()))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_headers(Any)
                .allow_methods(Any),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], state.config().port()));
    eprintln!("starting server on {}", addr);

    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("bind failed: {}", err);
            std::process::exit(1);
        }
    };

    axum::serve(listener, app).await.expect("serve error");
}
