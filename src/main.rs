use clap::Parser;
use logb::config::Config;
use std::future::IntoFuture;
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), logb::db::BoxError> {
    let config = Config::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(config.log.clone()))
        .init();
    if let Some(dest) = config.backup.clone() {
        logb::backup::run_once(&config.database_url()?, &dest).await?;
        println!("database backed up to {}", dest.display());
        return Ok(());
    }
    if let Some(src) = config.restore.clone() {
        let report = logb::restore::run(&config.database_url()?, &src).await?;
        println!("restored {} into {}", src.display(), report.data_dir.display());
        if let Some(kept) = report.replaced_to {
            println!("the database it replaced is kept at {}", kept.display());
        }
        println!("sync epoch is now {} -- every device will re-bootstrap", report.epoch);
        return Ok(());
    }
    if let Some(dest) = config.copy_to.clone() {
        let report = logb::copy::run(&config.database_url()?, &dest).await?;
        for (table, rows) in &report.tables {
            println!("{rows:>7} {table}");
        }
        println!("sync epoch is now {} -- every device will re-bootstrap", report.epoch);
        println!("blobs are NOT copied: copy the files/ directory alongside this database");
        return Ok(());
    }
    if config.healthcheck {
        return healthcheck(config.port).await;
    }
    let addr = format!("{}:{}", config.bind, config.port);
    let (app, state) = logb::build_with_state(config).await?;
    let tasks = logb::tasks::spawn(state.clone());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("LogB listening on http://{addr}");
    tokio::spawn(cancel_on_signal(state.shutdown.clone()));
    let shutdown = state.shutdown.clone();
    let mut serving = tokio::spawn(
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown.clone().cancelled_owned())
        .into_future(),
    );
    // Biased, cancellation first: once the token is cancelled the graceful server can finish
    // at the same moment, and an unbiased pick would sometimes take that branch and skip the
    // drain below. The server ending while nobody asked it to is a failure to report.
    tokio::select! {
        biased;
        () = shutdown.cancelled() => {}
        ended = &mut serving => {
            ended??;
            return Err("the server stopped without being asked to".into());
        }
    }

    // From here on everything shares one deadline: what is still running when it passes is
    // abandoned, because whoever sent the signal will not wait forever either (`docker stop`
    // sends SIGKILL ten seconds after SIGTERM).
    let deadline = tokio::time::Instant::now() + DRAIN;
    tracing::info!(drain_secs = DRAIN.as_secs(), "shutting down: no new connections; letting requests in flight finish");
    let drained = match tokio::time::timeout_at(deadline, &mut serving).await {
        Ok(ended) => {
            ended??;
            true
        }
        Err(_) => {
            tracing::warn!("requests still running after the drain deadline; stopping anyway");
            serving.abort();
            false
        }
    };
    let settled = tokio::time::timeout_at(deadline, tasks).await.is_ok();
    if !settled {
        tracing::warn!("the background loop was still busy at the drain deadline; stopping anyway");
    }
    // Closing waits for every connection to come back to its pool, which a request or a tick
    // abandoned above never does -- so only after both drained, and even then under the same
    // deadline. A clean close is what lets SQLite fold its WAL back into the database file
    // before the process goes.
    if drained && settled {
        let closed = tokio::time::timeout_at(deadline, async {
            state.write_db.close().await;
            state.db.close().await;
        })
        .await;
        if closed.is_err() {
            tracing::warn!("a connection was still in use at the drain deadline; stopping anyway");
        }
    }
    tracing::info!("stopped");
    Ok(())
}

/// How long a shutdown waits for requests in flight and the background loop, together.
const DRAIN: std::time::Duration = std::time::Duration::from_secs(10);

/// Cancels `shutdown` on SIGTERM or ctrl-c.
///
/// SIGTERM is what `docker stop`, systemd and Kubernetes send. This matters more than it looks
/// in the container: the binary is PID 1 in a scratch image, and PID 1 has no default action
/// for SIGTERM -- without a handler the signal was ignored and every `docker stop` waited out
/// its ten seconds and ended in SIGKILL, mid-request or mid-snapshot.
///
/// A handler that cannot be installed is logged and waited on forever rather than treated as a
/// signal: an instance that cannot hear SIGTERM is no worse off than before, while one that
/// shut itself down at startup would be down.
async fn cancel_on_signal(shutdown: tokio_util::sync::CancellationToken) {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!(error = %e, "cannot listen for ctrl-c");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => tracing::info!("ctrl-c received"),
        () = terminate => tracing::info!("SIGTERM received"),
    }
    shutdown.cancel();
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
