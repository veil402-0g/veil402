//! KMS, reached from somewhere with no network.
//!
//! The enclave's only channel is vsock. Bytes go to a proxy on the parent
//! which forwards them to KMS and forwards the answer back — but TLS is
//! established in here, against roots compiled into the image, so what the
//! parent relays is ciphertext it can neither read nor alter.
//!
//! `aws-sdk-kms` does the signing. That is not laziness: an attestation
//! mismatch and a transport failure both look like a refusal, and the SDK is
//! what tells them apart. On first light that distinction is the difference
//! between a wrong key policy and a proxy that is not running.

use std::sync::Arc;

use aws_sdk_kms::{
    config::{BehaviorVersion, Credentials, Region},
    primitives::Blob,
    types::{KeyAgreementAlgorithmSpec, KeyEncryptionMechanism, RecipientInfo},
};
use aws_smithy_runtime_api::client::{
    http::{HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector},
    orchestrator::{HttpRequest, HttpResponse},
    result::ConnectorError,
    runtime_components::RuntimeComponents,
};
use aws_smithy_types::body::SdkBody;
use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;
use tokio_rustls::{TlsConnector, client::TlsStream};
use tokio_vsock::{VsockAddr, VsockStream};

/// The parent always answers on this context id.
pub const PARENT_CID: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum KmsError {
    /// The gate said no. This is the interesting one: the measurements did
    /// not match the key policy.
    #[error("KMS refused to derive: {0}")]
    Denied(String),
    /// Never reached KMS at all — the proxy, the socket, or TLS.
    #[error("KMS was not reachable: {0}")]
    Transport(String),
    #[error("KMS answered unexpectedly: {0}")]
    Unexpected(String),
}

/// What the parent lends the enclave so it can sign a request.
///
/// Credentials only. They authorise calling KMS; the key policy is what
/// decides whether the call is answered, and no credential satisfies it
/// without the attestation.
#[derive(Clone, Debug)]
pub struct Lent {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
}

/// A smithy transport that tunnels each request over vsock.
///
/// Cloned per request and per retry by the SDK, so everything costly sits
/// behind an `Arc` rather than being rebuilt.
#[derive(Clone, Debug)]
struct OverVsock {
    port: u32,
    host: Arc<str>,
    tls: Arc<rustls::ClientConfig>,
}

impl HttpClient for OverVsock {
    fn http_connector(
        &self,
        _settings: &HttpConnectorSettings,
        _components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        SharedHttpConnector::new(self.clone())
    }
}

impl HttpConnector for OverVsock {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let this = self.clone();
        HttpConnectorFuture::new(Box::pin(async move {
            this.send(request)
                .await
                .map_err(|error| ConnectorError::other(error.into(), None))
        }))
    }
}

impl OverVsock {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, KmsError> {
        let prepared = self.prepare(request)?;
        let tls = self.connect().await?;
        exchange(prepared, tls).await
    }

    /// Adapts the SDK's request, and refuses any host but the one expected.
    ///
    /// The parent chooses where its proxy points, so the hostname is checked
    /// here as well as being the SNI: a redirected proxy cannot quietly widen
    /// who the enclave is talking to.
    fn prepare(&self, request: HttpRequest) -> Result<http::Request<SdkBody>, KmsError> {
        let mut request: http::Request<SdkBody> = request
            .try_into_http1x()
            .map_err(|error| KmsError::Unexpected(error.to_string()))?;
        let host = request
            .uri()
            .host()
            .ok_or_else(|| KmsError::Unexpected("the request named no host".into()))?;
        if host != &*self.host {
            return Err(KmsError::Unexpected(format!(
                "refusing to talk to {host}, which is not {}",
                self.host
            )));
        }
        // hyper's low-level client would otherwise send an absolute request
        // line and a second Host header, and KMS answers that with a bare 400.
        let path = request
            .uri()
            .path_and_query()
            .map_or_else(|| "/".to_owned(), |pq| pq.as_str().to_owned());
        *request.uri_mut() = path
            .parse()
            .map_err(|_| KmsError::Unexpected("the path will not parse".into()))?;
        // SigV4 signed the Host header but leaves the transport to add it,
        // inferring it from an authority that has just been removed.
        request.headers_mut().insert(
            http::header::HOST,
            http::HeaderValue::from_str(&self.host)
                .map_err(|_| KmsError::Unexpected("the host is not a header value".into()))?,
        );
        Ok(request)
    }

    async fn connect(&self) -> Result<TlsStream<VsockStream>, KmsError> {
        let socket = VsockStream::connect(VsockAddr::new(PARENT_CID, self.port))
            .await
            .map_err(|error| {
                KmsError::Transport(format!(
                    "no proxy answering on {PARENT_CID}:{}: {error}",
                    self.port
                ))
            })?;
        let name = rustls::pki_types::ServerName::try_from(self.host.to_string())
            .map_err(|_| KmsError::Unexpected("the host is not a server name".into()))?;
        TlsConnector::from(self.tls.clone())
            .connect(name, socket)
            .await
            .map_err(|error| KmsError::Transport(format!("TLS to {}: {error}", self.host)))
    }
}

async fn exchange(
    request: http::Request<SdkBody>,
    tls: TlsStream<VsockStream>,
) -> Result<HttpResponse, KmsError> {
    let (mut sender, driver) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
        .await
        .map_err(|error| KmsError::Transport(error.to_string()))?;
    tokio::spawn(async move {
        let _ = driver.await;
    });

    let response = sender
        .send_request(request)
        .await
        .map_err(|error| KmsError::Transport(error.to_string()))?;
    let (parts, body) = response.into_parts();
    let bytes = body
        .collect()
        .await
        .map_err(|error| KmsError::Transport(error.to_string()))?
        .to_bytes();
    if !parts.status.is_success() {
        // The SDK turns an unmodelled error body into an opaque `Unhandled`,
        // which on first light is the least useful thing it could say.
        tracing::error!(status = %parts.status, body = %String::from_utf8_lossy(&bytes), "KMS refused");
    }
    http::Response::from_parts(parts, SdkBody::from(bytes))
        .try_into()
        .map_err(|_| KmsError::Unexpected("the answer will not adapt".into()))
}

/// KMS, bound to the enclave's transport.
pub struct Kms {
    inner: aws_sdk_kms::Client,
}

impl Kms {
    /// # Errors
    ///
    /// Returns [`KmsError`] when the trust store will not build.
    pub fn open(region: &str, port: u32, lent: Lent) -> Result<Self, KmsError> {
        let transport = OverVsock {
            port,
            host: Arc::from(format!("kms.{region}.amazonaws.com")),
            tls: roots()?,
        };
        // Never `load_from_env`: the default provider chain reaches for IMDS
        // over a network that does not exist here, and hangs.
        let config = aws_sdk_kms::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region.to_owned()))
            .credentials_provider(Credentials::new(
                lent.access_key_id,
                lent.secret_access_key,
                Some(lent.session_token),
                None,
                "lent-by-the-parent",
            ))
            .http_client(transport)
            .sleep_impl(aws_smithy_async::rt::sleep::TokioSleep::new())
            .build();
        Ok(Self {
            inner: aws_sdk_kms::Client::from_conf(config),
        })
    }

    /// Agrees a secret against the peer point, wrapped to this boot's key.
    ///
    /// Returns the CMS envelope rather than the secret: supplying a recipient
    /// is what obliges KMS to withhold the plaintext, and a response that
    /// carried it anyway would mean the request was not the one intended.
    ///
    /// # Errors
    ///
    /// Returns [`KmsError::Denied`] when the measurements do not satisfy the
    /// key policy — which is the gate doing its job.
    pub async fn agree(
        &self,
        key: &str,
        peer_spki: &[u8],
        attestation: &[u8],
    ) -> Result<Vec<u8>, KmsError> {
        let answered = self
            .inner
            .derive_shared_secret()
            // An ARN, never an alias: an alias is a pointer someone else can move.
            .key_id(key)
            .key_agreement_algorithm(KeyAgreementAlgorithmSpec::Ecdh)
            .public_key(Blob::new(peer_spki.to_vec()))
            .recipient(
                RecipientInfo::builder()
                    .key_encryption_algorithm(KeyEncryptionMechanism::RsaesOaepSha256)
                    .attestation_document(Blob::new(attestation.to_vec()))
                    .build(),
            )
            .send()
            .await
            .map_err(|error| classify(&error))?;

        if answered.shared_secret().is_some() {
            return Err(KmsError::Unexpected(
                "KMS returned the secret in the clear despite a recipient".into(),
            ));
        }
        Ok(answered
            .ciphertext_for_recipient()
            .ok_or_else(|| KmsError::Unexpected("the answer wrapped nothing".into()))?
            .as_ref()
            .to_vec())
    }
}

/// Roots compiled into the image. The enclave trusts no store the parent
/// could have written to.
fn roots() -> Result<Arc<rustls::ClientConfig>, KmsError> {
    let mut store = rustls::RootCertStore::empty();
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // The provider is passed rather than defaulted: rustls panics when none is
    // installed, and a panic in here is a boot that never says why.
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| KmsError::Unexpected(error.to_string()))?
    .with_root_certificates(store)
    .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Keeps apart the two refusals that matter: the gate, and the wire.
fn classify<E: std::fmt::Debug>(error: &aws_sdk_kms::error::SdkError<E>) -> KmsError {
    use aws_sdk_kms::error::SdkError;
    match error {
        SdkError::ServiceError(inner) => KmsError::Denied(format!("{:?}", inner.err())),
        SdkError::DispatchFailure(inner) => KmsError::Transport(format!("{inner:?}")),
        other => KmsError::Unexpected(format!("{other:?}")),
    }
}
