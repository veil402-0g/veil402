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
    #[error("loopback would not come up: {0}")]
    Loopback(String),
}

/// Raises `lo`, which the enclave's init leaves down.
///
/// A down interface still accepts a bind, so the listener looks healthy and
/// only a connection fails, as ENETUNREACH. Through sysfs rather than an
/// ioctl: the flags are a file, and this workspace forbids unsafe.
///
/// # Errors
///
/// Returns [`RelayError::Loopback`] when the interface cannot be raised.
#[cfg(target_os = "linux")]
pub fn loopback() -> Result<(), RelayError> {
    const FLAGS: &str = "/sys/class/net/lo/flags";
    const UP: u32 = 0x1;

    let fail = |error: std::io::Error| RelayError::Loopback(error.to_string());
    let read = std::fs::read_to_string(FLAGS).map_err(fail)?;
    let held = u32::from_str_radix(read.trim().trim_start_matches("0x"), 16)
        .map_err(|_| RelayError::Loopback(format!("{FLAGS} does not read as flags")))?;
    if held & UP != 0 {
        return Ok(());
    }
    std::fs::write(FLAGS, format!("0x{:x}", held | UP)).map_err(fail)
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
