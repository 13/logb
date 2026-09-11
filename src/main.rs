use clap::Parser;
use logb::config::Config;
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), logb::db::BoxError> {
    let config = Config::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(config.log.clone()))
        .init();
    if let Some(dest) = config.backup.clone() {
        let pool = logb::db::connect_existing(&config.database_url()).await?;
        logb::db::backup_to(&pool, &dest).await?;
        println!("database backed up to {}", dest.display());
        return Ok(());
    }
    if let Some(src) = config.restore.clone() {
        let report = logb::restore::run(&config.data_dir, &src).await?;
        println!("restored {} into {}", src.display(), config.data_dir.display());
        if let Some(kept) = report.replaced_to {
            println!("the database it replaced is kept at {}", kept.display());
        }
        println!("sync epoch is now {} -- every device will re-bootstrap", report.epoch);
        return Ok(());
    }
    if config.healthcheck {
        return healthcheck(config.port).await;
    }
    let addr = format!("{}:{}", config.bind, config.port);
    let (app, state) = logb::build_with_state(config).await?;
    logb::tasks::spawn(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("LogB listening on http://{addr}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Probes a running instance over loopback. Used as the container HEALTHCHECK, where there is
/// no shell and no curl to run one with.
async fn healthcheck(port: u16) -> Result<(), logb::db::BoxError> {
    let url = format!("http://127.0.0.1:{port}/api/health");
    let res = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?
        .get(&url)
        .send()
        .await?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("health check failed: {url} returned {}", res.status()).into())
    }
}
