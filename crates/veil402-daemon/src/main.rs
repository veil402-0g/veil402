//! Runs the Veil gateway.

use std::{env, error::Error};

use alloy::hex;
use veil402_daemon::app::{Config, Daemon};
use veil402_sdk::Owner;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Read before the subscriber exists, so RUST_LOG can come from the file too.
    let environment = match dotenvy::dotenv() {
        Ok(path) => Some(path),
        Err(error) if error.not_found() => None,
        Err(error) => return Err(error.into()),
    };

    if env::args().nth(1).as_deref() == Some("keygen") {
        return keygen();
    }

    tracing_subscriber::fmt::init();
    if let Some(path) = environment {
        tracing::debug!(?path, "read environment file");
    }

    let daemon = Daemon::start(Config::from_env()?).await?;
    tracing::info!(leaves = daemon.index().leaves().await, "daemon started");
    daemon.run().await?;
    tracing::info!("daemon stopped");
    Ok(())
}

/// Prints a fresh owner. Most random 32-byte strings are not canonical field
/// elements, so these cannot be made up by hand.
fn keygen() -> Result<(), Box<dyn Error>> {
    let owner = Owner::random()?;
    println!("VEIL_SPEND_KEY={}", hex::encode(owner.spend.to_bytes()));
    println!("VEIL_VIEW_KEY={}", hex::encode(owner.view.to_bytes()));
    Ok(())
}
