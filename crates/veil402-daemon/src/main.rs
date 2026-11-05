//! Runs the Veil gateway.

use std::error::Error;

use veil402_daemon::app::{Config, Daemon};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Read before the subscriber exists, so RUST_LOG can come from the file too.
    let environment = match dotenvy::dotenv() {
        Ok(path) => Some(path),
        Err(error) if error.not_found() => None,
        Err(error) => return Err(error.into()),
    };
    tracing_subscriber::fmt::init();
    if let Some(path) = environment {
        tracing::debug!(?path, "read environment file");
    }

    let daemon = Daemon::start(&Config::from_env()?).await?;
    tracing::info!(leaves = daemon.index().leaves().await, "daemon started");
    daemon.run().await?;
    tracing::info!("daemon stopped");
    Ok(())
}
