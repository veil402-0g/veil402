//! A key that sends pool calls for other people.
//!
//! Lock and settle are answerable to a proof or the enclave's signature, not
//! to whoever sent them, so a buyer need not put its own address on chain to
//! make them. This holds the key that does instead.

mod http;
mod relay;

use std::{env, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};

use alloy::primitives::Address;
use tokio::{net::TcpListener, signal};
use tower_http::trace::TraceLayer;

use relay::Relay;

/// Gwei, in wei.
const GWEI: u128 = 1_000_000_000;
const MAX_FEE_GWEI: u128 = 400;

#[derive(Debug, thiserror::Error)]
enum ConfigError {
    #[error("{0} is not set")]
    Missing(&'static str),
    #[error("{0} is not valid")]
    Invalid(&'static str),
}

pub struct Settings {
    pub rpc: String,
    pub key: String,
    pub pool: Address,
    pub chain_id: u64,
    pub confirmations: u64,
    pub fee_cap: u128,
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::Missing(name))
}

fn parsed<T: FromStr>(name: &'static str, fallback: T) -> Result<T, ConfigError> {
    match env::var(name) {
        Err(_) => Ok(fallback),
        Ok(value) => value.parse().map_err(|_| ConfigError::Invalid(name)),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "veil402_relay=info,tower_http=warn".into()),
        )
        .init();

    let settings = Settings {
        rpc: required("VEIL_RPC_URL")?,
        key: required("VEIL_RELAY_KEY")?,
        pool: required("VEIL_POOL")?
            .parse()
            .map_err(|_| ConfigError::Invalid("VEIL_POOL"))?,
        chain_id: parsed("VEIL_CHAIN_ID", 296)?,
        confirmations: parsed("VEIL_CONFIRMATIONS", 0)?,
        fee_cap: parsed("VEIL_MAX_FEE_GWEI", MAX_FEE_GWEI)? * GWEI,
    };
    let state: PathBuf = parsed("VEIL_RELAY_STATE", PathBuf::from("relay-wallet.redb"))?;
    let handles: PathBuf = parsed("VEIL_RELAY_HANDLES", PathBuf::from("relay-handles.redb"))?;
    let bind: SocketAddr = parsed("VEIL_RELAY_BIND", "127.0.0.1:8403".parse()?)?;

    let relay = Arc::new(Relay::open(&settings, &state, &handles)?);
    tracing::info!(account = %relay.account(), pool = %settings.pool, %bind, "relaying");

    let app = http::router(relay).layer(TraceLayer::new_for_http());
    axum::serve(TcpListener::bind(bind).await?, app)
        .with_graceful_shutdown(async {
            signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
