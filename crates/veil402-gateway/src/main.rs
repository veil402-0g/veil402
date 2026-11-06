//! Serves the enclave to buyers.

use veil402_gateway::app::{Config, serve};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read before the subscriber exists, so RUST_LOG can come from the file too.
    let environment = match dotenvy::dotenv() {
        Ok(path) => Some(path),
        Err(error) if error.not_found() => None,
        Err(error) => return Err(error.into()),
    };
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    if let Some(path) = environment {
        tracing::debug!(?path, "read environment file");
    }
    Ok(serve(Config::from_env()?).await?)
}
