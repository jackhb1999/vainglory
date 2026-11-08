//! 匹配服务入口

mod queue;
mod routes;
mod store;

use tokio::net::TcpListener;
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fmt::init();

    let app = routes::build_router();
    let listener = TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("Matchmaking listening on :8080");
    axum::serve(listener, app).await?;
    Ok(())
}
