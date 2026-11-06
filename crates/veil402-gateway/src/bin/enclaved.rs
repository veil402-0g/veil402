//! The meter, as it runs inside an enclave.
//!
//! Everything this process needs arrives over vsock or is measured into the
//! image. It has no network, no disk worth the name, and no way in but the
//! one port below — which is the point: the seller's own host cannot read a
//! prompt, and neither can anyone who takes the host.
//!
//! The seed is not configured. It is agreed with KMS against a point fixed by
//! the image, which means it is the same on every boot and exists nowhere at
//! rest. The parent lends the credentials to ask; the key policy decides
//! whether the answer comes back.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the enclave only runs on linux");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    use std::sync::Arc;

    use tokio::sync::OnceCell;
    use veil402_enclave::{
        adapters::{nsm, relay},
        protocol::{Fault, Reply, Request, recv, send},
    };
    use veil402_gateway::{
        adapters::link::{Duplex, answer},
        app::ENCLAVE_PORT,
        core::Meters,
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Before anything trusts the RNG with a wrapping key.
    if let Err(error) = nsm::entropy_is_live() {
        tracing::error!(%error, "refusing to start");
        return std::process::ExitCode::FAILURE;
    }

    // Before anything binds or dials it: the interface is down until this
    // runs, and a bind on a down interface still succeeds, so the omission
    // would surface much later as an upstream that cannot be reached.
    if let Err(error) = relay::loopback() {
        tracing::error!(%error, "refusing to start");
        return std::process::ExitCode::FAILURE;
    }

    // The hop the upstream client resolves through. Started first so a
    // bootstrap that needs it does not race it.
    tokio::spawn(async move {
        if let Err(error) = relay::outbound(enclaved::UPSTREAM_HOP, enclaved::UPSTREAM_PORT).await {
            tracing::error!(%error, "outbound traffic has nowhere to go");
        }
    });

    let built: Arc<enclaved::Built> = Arc::new(enclaved::Built::default());
    let mut listener = match tokio_vsock::VsockListener::bind(tokio_vsock::VsockAddr::new(
        0xFFFF_FFFF,
        ENCLAVE_PORT,
    )) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, port = ENCLAVE_PORT, "nothing can reach this enclave");
            return std::process::ExitCode::FAILURE;
        }
    };
    tracing::info!(port = ENCLAVE_PORT, "the enclave is listening");

    loop {
        let Ok((socket, _)) = listener.accept().await else {
            continue;
        };
        let built = built.clone();
        tokio::spawn(async move {
            let mut link: std::pin::Pin<Box<dyn Duplex>> = Box::pin(socket);
            // Peeking at the first message is what lets bootstrap be handled
            // here while everything else goes to the shared dispatch.
            let request: Request = match recv(&mut link).await {
                Ok(request) => request,
                Err(error) => {
                    tracing::warn!(%error, "a caller said nothing usable");
                    return;
                }
            };
            let answered = match request {
                Request::Bootstrap(lent) => built.reconcile(*lent).await,
                other => {
                    let Some(built) = built.meters() else {
                        // Not something a retry fixes: the caller has to
                        // bootstrap before there is a meter to ask.
                        let _ = send(&mut link, &Reply::Failed(Fault::Unknown)).await;
                        return;
                    };
                    if let Err(error) = answer(&built, other, link).await {
                        tracing::warn!(%error, "a call did not finish");
                    }
                    return;
                }
            };
            let _ = send(&mut link, &answered).await;
        });
    }
}

#[cfg(target_os = "linux")]
mod enclaved {
    use std::{collections::HashMap, path::PathBuf, sync::Arc, sync::Mutex};

    use alloy::primitives::Address;
    use veil402_enclave::{
        adapters::{
            Router,
            kms::{Kms, Lent as Credentials},
            seed,
        },
        protocol::{Fault, Lent, Reply, Settings},
    };
    use veil402_gateway::{
        adapters::Enclave,
        core::{Meters, Pricing},
    };
    use veil402_sdk::{
        Config as Proving, Veil,
        evm::PoolClient,
        session::{Keyring, RootSeed},
    };

    /// Where the upstream client is pointed. Loopback, because the enclave
    /// has one and nothing else, and on the port the upstream URL names:
    /// the client is aimed here by resolution, and resolution carries no
    /// port, so the one in the URL is what it dials.
    pub const UPSTREAM_HOP: &str = "127.0.0.1:443";
    /// Where the parent relays that traffic onward.
    pub const UPSTREAM_PORT: u32 = 8443;
    /// Where the parent relays KMS traffic.
    pub const KMS_PORT: u32 = 8000;

    /// What the image was built expecting its seed to be.
    ///
    /// Empty is bootstrap: the first image reports the commitment so the next
    /// can be built with it. Shipping it empty leaves the gate open, and an
    /// open gate is a wrong seed serving quietly.
    /// Absent at build time means bootstrap: the commitment is reported and
    /// the gate stays open, which is only correct for the first image.
    const EXPECTED_SEED: &str = match option_env!("VEIL_EXPECTED_SEED") {
        Some(pinned) => pinned,
        None => "",
    };

    /// Where the prover and its keys sit in the image. Fixed rather than
    /// lent: they are measured, and a path the parent could choose would be
    /// a prover the parent could choose.
    const WORKER: &str = "/usr/local/bin/veil402-gnark";
    const ARTIFACTS: &str = "/opt/veil402/artifacts/transaction-v4";

    /// What one successful build left behind.
    struct Serving {
        settings: Settings,
        meters: Arc<dyn Meters>,
        /// Reported on every posting, not just the one that built, so the
        /// parent can keep checking the seed is still the measured one.
        commitment: String,
    }

    /// The meter, and the settings it was built for.
    #[derive(Default)]
    pub struct Built {
        /// Held across the whole build so two postings cannot race into two
        /// meters, each having derived the seed separately.
        state: Mutex<Option<Serving>>,
    }

    impl Built {
        /// The meter, if there is one yet.
        pub fn meters(&self) -> Option<Arc<dyn Meters>> {
            self.state
                .lock()
                .ok()
                .and_then(|held| held.as_ref().map(|serving| serving.meters.clone()))
        }

        /// Takes a posting: builds on the first, agrees with the rest.
        ///
        /// Settings are pinned by the first one that works. A later posting
        /// that disagrees is refused and said out loud — it is either a
        /// misconfiguration or a parent trying to repoint an enclave that
        /// has already been attested to, and neither should be applied.
        pub async fn reconcile(&self, lent: Lent) -> Reply {
            if let Ok(held) = self.state.lock() {
                if let Some(serving) = held.as_ref() {
                    return if serving.settings == lent.settings {
                        Reply::Ready { commitment: serving.commitment.clone() }
                    } else {
                        tracing::error!(
                            "a posting asked this enclave to serve a different pool; refusing"
                        );
                        Reply::Failed(Fault::Refused(
                            "this enclave is already serving other settings".into(),
                        ))
                    };
                }
            }

            match build(&lent).await {
                Ok((meters, commitment)) => {
                    if let Ok(mut held) = self.state.lock() {
                        // Another posting may have won the race while this
                        // one was deriving; the first to arrive is the one
                        // that counts.
                        if held.is_none() {
                            *held = Some(Serving {
                                settings: lent.settings,
                                meters,
                                commitment: commitment.clone(),
                            });
                        }
                    }
                    Reply::Ready { commitment }
                }
                Err(why) => {
                    tracing::warn!(%why, "the enclave could not be built yet");
                    Reply::Failed(Fault::Refused(why))
                }
            }
        }
    }

    async fn build(lent: &Lent) -> Result<(Arc<dyn Meters>, String), String> {
        let kms = Kms::open(
            &lent.region,
            lent.kms_port,
            Credentials {
                access_key_id: lent.access_key_id.clone(),
                secret_access_key: lent.secret_access_key.clone(),
                session_token: lent.session_token.clone(),
            },
        )
        .map_err(|error| error.to_string())?;
        let seed = seed::load(&kms, &lent.key, EXPECTED_SEED)
            .await
            .map_err(|error| error.to_string())?;
        let commitment = hex(&seed.commitment());

        let settings = &lent.settings;
        let pool = PoolClient::new(
            Address::from(settings.pool),
            Address::from(settings.token),
            Address::from(settings.verifier),
            settings.chain_id,
        )
        .map_err(|_| "the pool addresses do not make a client".to_owned())?;

        let veil = Arc::new(
            Veil::open(Proving::new(PathBuf::from(WORKER), PathBuf::from(ARTIFACTS)))
                .map_err(|_| "the proving artifacts are not in this image".to_owned())?,
        );

        // Reached through the loopback hop, so TLS still terminates here.
        let through = UPSTREAM_HOP
            .parse()
            .map_err(|_| "the local hop is not an address".to_owned())?;
        let upstream = Arc::new(
            Router::via(
                &settings.upstream,
                &settings.upstream_key,
                &settings.trust,
                settings.ceiling,
                // Empty: nothing in here reads the serving contract yet, and
                // the port's own note is that a registry which is away costs
                // the attestation claim rather than the service.
                Arc::new(HashMap::new()),
                Some(through),
            )
            .map_err(|_| "the upstream will not build".to_owned())?,
        );

        let enclave = Enclave::new(
            pool,
            Keyring::new(RootSeed::from_bytes(seed.expose())),
            veil,
            upstream,
            Pricing::new(settings.scale, settings.margin_bps),
        )
        .map_err(|error| error.to_string())?;
        tracing::info!(signer = %enclave.signer(), %commitment, "the enclave is serving");
        Ok((Arc::new(enclave), commitment))
    }

    fn hex(bytes: &[u8; 32]) -> String {
        bytes.iter().fold(String::with_capacity(64), |mut out, byte| {
            use std::fmt::Write as _;
            let _ = write!(out, "{byte:02x}");
            out
        })
    }
}
