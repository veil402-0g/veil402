//! Telling the enclave what it is for, on a timer.
//!
//! Not a handshake. The enclave holds nothing durable, so one that restarts
//! comes back empty and would stay empty until somebody noticed. Posting on
//! a clock means it is serving again within the interval, and the parent
//! never has to detect that it went.
//!
//! Which makes this reconciliation rather than a command: every posting says
//! the same thing, and the enclave answers the same way whether it built
//! something or found it already built.

use std::time::Duration;

use veil402_enclave::protocol::{Lent, Reply, Request, Settings, recv, send};

use super::link::{Connect, Duplex};
use crate::app::Config;

/// How often the parent repeats itself. A restarted enclave is dark for at
/// most this long, and the posting costs one vsock round trip.
const EVERY: Duration = Duration::from_mins(1);

/// Credentials the parent can lend. They authorise asking KMS, and nothing
/// else: the key policy is what decides whether the answer comes back, and
/// no credential satisfies it without the attestation.
pub struct Borrowed {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
}

/// Posts until the enclave is built, and keeps posting after.
///
/// Never returns. Errors are logged and retried on the next tick: an enclave
/// that is not up yet is the normal case at boot, not a failure.
pub async fn keep<C: Connect>(connect: C, config: &Config, key: String, region: String, kms_port: u32) -> ! {
    let settings = settings(config);
    let mut serving = false;
    loop {
        match post(&connect, &settings, &key, &region, kms_port).await {
            Ok(commitment) => {
                if !serving {
                    tracing::info!(%commitment, "the enclave is serving");
                    serving = true;
                }
            }
            Err(why) => {
                // Loud the first time, quiet after: an enclave that is down
                // for an hour should not write an hour of identical lines.
                if serving {
                    tracing::warn!(%why, "the enclave stopped answering");
                    serving = false;
                } else {
                    tracing::debug!(%why, "the enclave is not up yet");
                }
            }
        }
        tokio::time::sleep(EVERY).await;
    }
}

async fn post<C: Connect>(
    connect: &C,
    settings: &Settings,
    key: &str,
    region: &str,
    kms_port: u32,
) -> Result<String, String> {
    let borrowed = credentials().await?;
    let mut link: std::pin::Pin<Box<dyn Duplex>> =
        connect.connect().await.map_err(|error| error.to_string())?;
    let lent = Lent {
        access_key_id: borrowed.access_key_id,
        secret_access_key: borrowed.secret_access_key,
        session_token: borrowed.session_token,
        region: region.to_owned(),
        kms_port,
        key: key.to_owned(),
        settings: settings.clone(),
    };
    send(&mut link, &Request::Bootstrap(Box::new(lent)))
        .await
        .map_err(|error| error.to_string())?;
    match recv(&mut link).await.map_err(|error| error.to_string())? {
        Reply::Ready { commitment } => Ok(commitment),
        Reply::Failed(fault) => Err(format!("{fault:?}")),
        _ => Err("the enclave answered out of turn".to_owned()),
    }
}

/// What this host is allowed to ask KMS for, from the role it is running
/// under. Fetched per posting because they expire.
async fn credentials() -> Result<Borrowed, String> {
    use aws_credential_types::provider::ProvideCredentials as _;

    let chain = aws_config::default_provider::credentials::DefaultCredentialsChain::builder()
        .build()
        .await;
    let found = chain
        .provide_credentials()
        .await
        .map_err(|error| format!("this host has no credentials to lend: {error}"))?;
    Ok(Borrowed {
        access_key_id: found.access_key_id().to_owned(),
        secret_access_key: found.secret_access_key().to_owned(),
        session_token: found
            .session_token()
            .ok_or("the credentials carry no session token")?
            .to_owned(),
    })
}

/// What the enclave is for, drawn from the host's own configuration.
fn settings(config: &Config) -> Settings {
    Settings {
        pool: config.pool.address().into(),
        token: config.pool.token().into(),
        verifier: config.pool.verifier().into(),
        chain_id: config.pool.chain_id(),
        upstream: config.upstream.clone(),
        upstream_key: config.upstream_key.clone(),
        trust: config.trust.clone(),
        ceiling: config.ceiling,
        scale: config.scale,
        margin_bps: config.margin_bps,
    }
}
