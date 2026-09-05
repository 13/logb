use clap::Parser;
use memto::config::Config;
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), memto::db::BoxError> {
    let config = Config::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(config.log.clone()))
        .init();
    let addr = format!("{}:{}", config.bind, config.port);
    let (app, state) = memto::build_with_state(config).await?;
    memto::notify::spawn(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("memto listening on http://{addr}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
