use axum::{Json, Router, routing::get};
use serde::Serialize;
use std::net::SocketAddr;

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

#[derive(Serialize)]
struct Hello {
    message: &'static str,
}

async fn hello() -> Json<Hello> {
    Json(Hello { message: "hello from bluetext" })
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3030);

    let app = Router::new()
        .route("/", get(|| async { "bluetext api" }))
        .route("/health", get(health))
        .route("/hello", get(hello));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("api listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
