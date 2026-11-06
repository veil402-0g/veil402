//! Bytes in and out of a place with no network.
//!
//! An enclave reaches nothing but its parent, over vsock. So outbound traffic
//! takes two hops: a listener on the enclave's own loopback forwards to
//! vsock, and the parent forwards that to the real host.
//!
//! The reason this is a byte relay and not an HTTP proxy is that TLS has to
//! terminate inside the enclave. The client is pointed at the local hop by
//! resolution alone — it still speaks to the real hostname, still checks the
//! real certificate — so both hops carry ciphertext that neither can read.

use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
};

#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("{0} could not be listened on")]
    Bind(String),
}

/// Inside the enclave: loopback in, vsock out.
///
/// # Errors
///
/// Returns [`RelayError::Bind`] when the local address is unavailable.
#[cfg(target_os = "linux")]
pub async fn outbound(local: &str, parent_port: u32) -> Result<(), RelayError> {
    use tokio_vsock::{VsockAddr, VsockStream};

    let listener = TcpListener::bind(local)
        .await
        .map_err(|_| RelayError::Bind(local.to_owned()))?;
    tracing::info!(local, parent_port, "relaying outbound traffic");
    loop {
        let Ok((mut near, _)) = listener.accept().await else {
            continue;
        };
        tokio::spawn(async move {
            match VsockStream::connect(VsockAddr::new(super::kms::PARENT_CID, parent_port)).await {
                Ok(mut far) => {
                    let _ = copy_bidirectional(&mut near, &mut far).await;
                }
                Err(error) => tracing::warn!(%error, "the parent is not relaying"),
            }
        });
    }
}

/// On the parent: vsock in, TCP out.
///
/// The destination is fixed by the parent rather than named by the enclave.
/// An enclave that could choose where its bytes went would be an enclave that
/// could be told to send them somewhere else.
///
/// # Errors
///
/// Returns [`RelayError::Bind`] when the vsock port is unavailable.
#[cfg(target_os = "linux")]
pub async fn inbound(port: u32, upstream: &str) -> Result<(), RelayError> {
    use tokio_vsock::{VsockAddr, VsockListener};

    const ANY: u32 = 0xFFFF_FFFF;

    let mut listener = VsockListener::bind(VsockAddr::new(ANY, port))
        .map_err(|_| RelayError::Bind(format!("vsock:{port}")))?;
    tracing::info!(port, upstream, "relaying to the world");
    loop {
        let Ok((mut near, _)) = listener.accept().await else {
            continue;
        };
        let upstream = upstream.to_owned();
        tokio::spawn(async move {
            match TcpStream::connect(&upstream).await {
                Ok(mut far) => {
                    let _ = copy_bidirectional(&mut near, &mut far).await;
                }
                Err(error) => tracing::warn!(%error, upstream, "the world is not answering"),
            }
        });
    }
}
