//! The meter, across a process boundary.
//!
//! Both halves live here because they have to agree: once the enclave is
//! measured and signed, the two binaries ship separately and neither can be
//! corrected without the other noticing. Keeping the dispatch beside the
//! client is what makes a mismatch a compile error rather than a deployment.
//!
//! One connection per call. vsock connections are cheap, and a single
//! multiplexed channel would need correlation ids, a reader task and a
//! head-of-line story — none of which this workload is asking for.

use std::{pin::Pin, sync::Arc};

use async_trait::async_trait;
use futures_util::{StreamExt, stream::BoxStream};
use tokio::io::{AsyncRead, AsyncWrite};
use veil402_enclave::protocol::{Envelope, Fault, Kept as KeptWire, LinkError, Reply, Request, Reserved, recv, send};
use veil402_sdk::{Field, evm::Session, session::Sealed};

use crate::core::{
    Accepted, Billed, Kept, MeterError, Meters, Metering, Opened, Replayed,
};

/// Either direction of a duplex connection.
pub trait Duplex: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Duplex for T {}

/// Opens one connection to the meter.
///
/// A trait so the same client runs over vsock on hardware and over anything
/// duplex in a test; the protocol is not where the transport belongs.
#[async_trait]
pub trait Connect: Send + Sync {
    async fn connect(&self) -> Result<Pin<Box<dyn Duplex>>, LinkError>;
}

fn gone(_: LinkError) -> MeterError {
    MeterError::Refused("the enclave is not answering".into())
}

impl From<Fault> for MeterError {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Unknown => Self::Unknown,
            Fault::CeilingReached => Self::CeilingReached,
            // The crossing costs the 'static: the variants that matter are
            // kept apart, and only the words inside this one are rebuilt.
            Fault::Unreserved(why) => Self::Refused(format!("no escrow backs this session: {why}")),
            Fault::Repeated => Self::Repeated,
            Fault::Reused => Self::Reused,
            Fault::Refused(why) => Self::Refused(why),
            Fault::Upstream { status, retry_after, head, sealed } => {
                Self::Upstream { status, retry_after, head, sealed }
            }
        }
    }
}

impl From<&MeterError> for Fault {
    fn from(error: &MeterError) -> Self {
        match error {
            MeterError::Unknown => Self::Unknown,
            MeterError::CeilingReached => Self::CeilingReached,
            MeterError::Unreserved(why) => Self::Unreserved((*why).to_owned()),
            MeterError::Repeated => Self::Repeated,
            MeterError::Reused => Self::Reused,
            MeterError::Refused(why) => Self::Refused(why.clone()),
            MeterError::Upstream { status, retry_after, head, sealed } => Self::Upstream {
                status: *status,
                retry_after: *retry_after,
                head: *head,
                sealed: sealed.clone(),
            },
        }
    }
}

// ---------------------------------------------------------------- server

/// Answers one call, then lets the connection close.
///
/// # Errors
///
/// Returns [`LinkError`] when the connection drops mid-answer. A refusal by
/// the meter is an answer, not an error here.
pub async fn serve(meters: &Arc<dyn Meters>, mut link: Pin<Box<dyn Duplex>>) -> Result<(), LinkError> {
    let request: Request = recv(&mut link).await?;
    answer(meters, request, link).await
}

/// The same, for a caller that has already read the request.
///
/// The enclave's own loop peeks at the first message to catch `Bootstrap`,
/// which builds the meter this takes — so it cannot read it a second time.
///
/// # Errors
///
/// Returns [`LinkError`] when the connection drops mid-answer.
pub async fn answer(
    meters: &Arc<dyn Meters>,
    request: Request,
    mut link: Pin<Box<dyn Duplex>>,
) -> Result<(), LinkError> {
    match request {
        // The binary owns this one: it is what builds the meter this
        // function is handed, so by here it has already happened.
        Request::Bootstrap(_) => {
            send(&mut link, &Reply::Failed(Fault::Refused("already bootstrapped".into()))).await
        }
        Request::Signer => send(&mut link, &Reply::Signer(meters.signer().into())).await,
        Request::Catalogue => match meters.catalogue().await {
            Ok(offers) => send(&mut link, &Reply::Catalogue(offers)).await,
            Err(error) => send(&mut link, &Reply::Failed((&error).into())).await,
        },
        Request::Agree(session) => {
            let session = Session::try_from(session)?;
            match meters.agree(&session).await {
                Ok(body) => send(&mut link, &Reply::Agreed(body.into())).await,
                Err(error) => send(&mut link, &Reply::Failed((&error).into())).await,
            }
        }
        Request::Accept { sealed, session } => {
            let session = Session::try_from(session)?;
            match meters.accept(&Sealed::from(sealed), session).await {
                Ok(accepted) => {
                    send(&mut link, &Reply::Accepted { root: accepted.root.to_bytes() }).await
                }
                Err(error) => send(&mut link, &Reply::Failed((&error).into())).await,
            }
        }
        Request::Release(nullifier) => {
            meters.release(field(nullifier)?).await;
            send(&mut link, &Reply::Released).await
        }
        Request::Holds(nullifier) => {
            let held = meters.holds(field(nullifier)?).await;
            send(&mut link, &Reply::Holds(held)).await
        }
        Request::Resume(kept) => {
            let kept = Kept {
                sealed: Sealed::from(kept.sealed),
                session: Session::try_from(kept.session)?,
                log: kept.log,
            };
            match meters.resume(&kept).await {
                Ok(replayed) => {
                    let wire = replayed
                        .into_iter()
                        .map(|one| veil402_enclave::protocol::Replayed {
                            attempt: one.attempt,
                            total: one.total,
                            remaining: one.remaining,
                            attestation: one.attestation.0,
                        })
                        .collect();
                    send(&mut link, &Reply::Resumed(wire)).await
                }
                Err(error) => send(&mut link, &Reply::Failed((&error).into())).await,
            }
        }
        Request::Settle { nullifier, dust } => {
            match meters.settle(field(nullifier)?, dust).await {
                Ok(body) => send(&mut link, &Reply::Settled(body)).await,
                Err(error) => send(&mut link, &Reply::Failed((&error).into())).await,
            }
        }
        Request::Record { nullifier, attempt, sealed } => {
            let opened = meters
                .record(field(nullifier)?, &attempt, &Sealed::from(sealed))
                .await;
            match opened {
                Err(error) => send(&mut link, &Reply::Failed((&error).into())).await,
                Ok(Opened { head, mut frames }) => {
                    send(&mut link, &Reply::Opened { head }).await?;
                    while let Some(next) = frames.next().await {
                        let reply = match next {
                            Ok(Billed::Frame(bytes)) => Reply::Frame(bytes),
                            Ok(Billed::Bill(Metering { metered, record })) => {
                                Reply::Bill { metered: metered.into(), record }
                            }
                            // A stream that fails mid-answer says so and
                            // stops; the host has already relayed what came
                            // before and cannot unsay it.
                            Err(error) => {
                                send(&mut link, &Reply::Failed((&error).into())).await?;
                                return Ok(());
                            }
                        };
                        send(&mut link, &reply).await?;
                    }
                    send(&mut link, &Reply::Done).await
                }
            }
        }
    }
}

fn field(bytes: [u8; 32]) -> Result<Field, LinkError> {
    Field::from_bytes(bytes).map_err(|_| LinkError::Malformed)
}

// ---------------------------------------------------------------- client

/// The meter, reached over a link instead of called directly.
pub struct Linked<C> {
    connect: C,
    /// Asked once at startup: the trait reads it synchronously, and a
    /// round trip cannot happen behind a `fn`. It is also the one thing
    /// about an enclave that never changes while it is running.
    signer: alloy::primitives::Address,
}

impl<C: Connect> Linked<C> {
    /// Opens the link and learns which key answers on it.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when the enclave cannot be reached or answers
    /// something other than its signer.
    pub async fn open(connect: C) -> Result<Self, MeterError> {
        let mut link = connect.connect().await.map_err(gone)?;
        send(&mut link, &Request::Signer).await.map_err(gone)?;
        match recv(&mut link).await.map_err(gone)? {
            Reply::Signer(bytes) => Ok(Self { connect, signer: bytes.into() }),
            Reply::Failed(fault) => Err(fault.into()),
            _ => Err(MeterError::Refused("the enclave answered out of turn".into())),
        }
    }

    /// Sends one request and reads the single reply it expects.
    async fn call(&self, request: &Request) -> Result<Reply, MeterError> {
        let mut link = self.connect.connect().await.map_err(gone)?;
        send(&mut link, request).await.map_err(gone)?;
        match recv(&mut link).await.map_err(gone)? {
            Reply::Failed(fault) => Err(fault.into()),
            reply => Ok(reply),
        }
    }
}

/// What a reply that is not the one asked for means.
fn out_of_turn<T>() -> Result<T, MeterError> {
    Err(MeterError::Refused("the enclave answered out of turn".into()))
}

#[async_trait]
impl<C: Connect + 'static> Meters for Linked<C> {
    fn signer(&self) -> alloy::primitives::Address {
        self.signer
    }

    async fn catalogue(&self) -> Result<Vec<veil402_sdk::wire::Offer>, MeterError> {
        match self.call(&Request::Catalogue).await? {
            Reply::Catalogue(offers) => Ok(offers),
            _ => out_of_turn(),
        }
    }

    async fn agree(&self, session: &Session) -> Result<veil402_sdk::wire::AgreeBody, MeterError> {
        match self.call(&Request::Agree(Reserved::from(*session))).await? {
            Reply::Agreed(body) => Ok(body.into()),
            _ => out_of_turn(),
        }
    }

    async fn accept(&self, sealed: &Sealed, session: Session) -> Result<Accepted, MeterError> {
        let request = Request::Accept {
            sealed: Envelope::from(sealed),
            session: Reserved::from(session),
        };
        match self.call(&request).await? {
            Reply::Accepted { root } => Ok(Accepted {
                root: Field::from_bytes(root)
                    .map_err(|_| MeterError::Refused("the enclave named no root".into()))?,
            }),
            _ => out_of_turn(),
        }
    }

    async fn release(&self, nullifier: Field) {
        // Releasing is the host forgetting something it could not vouch for.
        // A link that is down has already forgotten it.
        let _ = self.call(&Request::Release(nullifier.to_bytes())).await;
    }

    async fn holds(&self, nullifier: Field) -> bool {
        matches!(
            self.call(&Request::Holds(nullifier.to_bytes())).await,
            Ok(Reply::Holds(true))
        )
    }

    async fn record(
        &self,
        nullifier: Field,
        attempt: &str,
        sealed: &Sealed,
    ) -> Result<Opened, MeterError> {
        let mut link = self.connect.connect().await.map_err(gone)?;
        let request = Request::Record {
            nullifier: nullifier.to_bytes(),
            attempt: attempt.to_owned(),
            sealed: Envelope::from(sealed),
        };
        send(&mut link, &request).await.map_err(gone)?;
        let head = match recv(&mut link).await.map_err(gone)? {
            Reply::Opened { head } => head,
            Reply::Failed(fault) => return Err(fault.into()),
            _ => return out_of_turn(),
        };
        // The connection is the stream: it is held until `Done`, and dropping
        // the stream closes it, which is what tells the enclave to stop.
        let frames = futures_util::stream::unfold(Some(link), |held| async move {
            let mut link = held?;
            match recv::<_, Reply>(&mut link).await {
                Ok(Reply::Frame(bytes)) => Some((Ok(Billed::Frame(bytes)), Some(link))),
                Ok(Reply::Bill { metered, record }) => {
                    let bill = Metering { metered: metered.into(), record };
                    Some((Ok(Billed::Bill(bill)), Some(link)))
                }
                Ok(Reply::Done) => None,
                Ok(Reply::Failed(fault)) => Some((Err(fault.into()), None)),
                Ok(_) => Some((out_of_turn::<Billed>(), None)),
                Err(error) => Some((Err(gone(error)), None)),
            }
        });
        Ok(Opened { head, frames: Box::pin(frames) as BoxStream<'static, _> })
    }

    async fn resume(&self, kept: &Kept) -> Result<Vec<Replayed>, MeterError> {
        let request = Request::Resume(KeptWire {
            sealed: Envelope::from(&kept.sealed),
            session: Reserved::from(kept.session),
            log: kept.log.clone(),
        });
        match self.call(&request).await? {
            Reply::Resumed(rows) => Ok(rows
                .into_iter()
                .map(|one| Replayed {
                    attempt: one.attempt,
                    total: one.total,
                    remaining: one.remaining,
                    attestation: one.attestation.into(),
                })
                .collect()),
            _ => out_of_turn(),
        }
    }

    async fn settle(
        &self,
        nullifier: Field,
        dust: bool,
    ) -> Result<veil402_sdk::wire::SettlementBody, MeterError> {
        let request = Request::Settle { nullifier: nullifier.to_bytes(), dust };
        match self.call(&request).await? {
            Reply::Settled(body) => Ok(body),
            _ => out_of_turn(),
        }
    }
}

// ---------------------------------------------------------------- vsock

/// Dials the enclave over vsock.
///
/// Only inside a Nitro parent: there is no such socket anywhere else, which
/// is also why the in-process meter has to remain available.
#[cfg(target_os = "linux")]
pub struct Vsock {
    pub cid: u32,
    pub port: u32,
}

#[cfg(target_os = "linux")]
#[async_trait]
impl Connect for Vsock {
    async fn connect(&self) -> Result<Pin<Box<dyn Duplex>>, LinkError> {
        let socket = tokio_vsock::VsockStream::connect(tokio_vsock::VsockAddr::new(
            self.cid, self.port,
        ))
        .await
        .map_err(|_| LinkError::Closed)?;
        Ok(Box::pin(socket))
    }
}

/// Answers calls from the parent until it stops asking.
///
/// One task per connection, and a connection is one call — so a slow answer
/// blocks nothing, and a caller that goes away closes its own.
///
/// # Errors
///
/// Returns [`LinkError`] when the listener cannot be bound.
#[cfg(target_os = "linux")]
pub async fn listen(meters: Arc<dyn Meters>, port: u32) -> Result<(), LinkError> {
    // Any context id: the parent addresses the enclave, not the reverse.
    const ANY: u32 = 0xFFFF_FFFF;

    let mut listener =
        tokio_vsock::VsockListener::bind(tokio_vsock::VsockAddr::new(ANY, port))
            .map_err(|_| LinkError::Closed)?;
    tracing::info!(port, "the meter is listening");

    loop {
        match listener.accept().await {
            Ok((socket, _)) => {
                let meters = meters.clone();
                tokio::spawn(async move {
                    if let Err(error) = serve(&meters, Box::pin(socket)).await {
                        tracing::warn!(%error, "a call did not finish");
                    }
                });
            }
            // One refused connection is not a reason to stop answering the
            // rest; a listener that cannot accept at all will say so again.
            Err(error) => tracing::warn!(%error, "a connection was refused"),
        }
    }
}
