//! Attestation, and the key the seed comes back wrapped to.
//!
//! KMS will only derive for an enclave that can prove what it is, and it
//! returns the result encrypted to a public key carried inside that proof. So
//! the keypair is minted here, from the enclave's own hardware entropy, once
//! per boot — never handed in by the parent, which would be handing the parent
//! the seed.
//!
//! The entropy check runs first. An unseeded RNG does not fail loudly; it
//! returns predictable bytes, and a predictable wrapping key is a readable
//! seed.

use aws_nitro_enclaves_nsm_api::{
    api::{Request, Response},
    driver::{nsm_exit, nsm_init, nsm_process_request},
};
use rand::{RngCore, rngs::OsRng};
use rsa::{RsaPrivateKey, pkcs8::EncodePublicKey};
use serde_bytes::ByteBuf;

/// Enough to wrap a 48-byte secret, small enough to keep the attestation
/// document short. The key lives for one boot.
const RSA_BITS: usize = 2048;

#[derive(Debug, thiserror::Error)]
pub enum AttestError {
    #[error("the enclave's randomness is not live: {0}")]
    Entropy(String),
    #[error("no wrapping key: {0}")]
    KeyGen(String),
    #[error("the security module refused: {0}")]
    Nsm(String),
}

/// A proof of what is running, and the private half of the key it names.
pub struct Attested {
    pub wrapping: RsaPrivateKey,
    pub document: Vec<u8>,
}

/// Rejects the outputs a dead RNG produces before anything depends on it.
///
/// # Errors
///
/// Returns [`AttestError::Entropy`] when randomness is unavailable or
/// degenerate.
pub fn entropy_is_live() -> Result<(), AttestError> {
    let mut first = [0_u8; 32];
    let mut second = [0_u8; 32];
    let unavailable =
        |error: rand::Error| AttestError::Entropy(format!("the system RNG would not answer: {error}"));
    OsRng.try_fill_bytes(&mut first).map_err(unavailable)?;
    OsRng.try_fill_bytes(&mut second).map_err(unavailable)?;
    if first == [0_u8; 32] || second == [0_u8; 32] {
        return Err(AttestError::Entropy("it returned zeros".into()));
    }
    if first == second {
        return Err(AttestError::Entropy("two draws came back the same".into()));
    }
    Ok(())
}

/// Mints a wrapping key and gets the security module to vouch for it.
///
/// # Errors
///
/// Returns [`AttestError`] when randomness is dead, the key will not
/// generate, or `/dev/nsm` is absent — the last of which means this is not an
/// enclave and must not be treated as one.
pub fn attest() -> Result<Attested, AttestError> {
    entropy_is_live()?;

    let wrapping = RsaPrivateKey::new(&mut OsRng, RSA_BITS)
        .map_err(|error| AttestError::KeyGen(error.to_string()))?;
    let spki = wrapping
        .to_public_key()
        .to_public_key_der()
        .map_err(|error| AttestError::KeyGen(error.to_string()))?
        .as_bytes()
        .to_vec();

    let device = nsm_init();
    if device < 0 {
        return Err(AttestError::Nsm(
            "/dev/nsm is absent, so this is not running inside an enclave".into(),
        ));
    }
    // Only the public key is bound in. Freshness comes from the key being new
    // this boot, and KMS matches on the measurements rather than a nonce.
    let asked = Request::Attestation {
        user_data: None,
        nonce: None,
        public_key: Some(ByteBuf::from(spki)),
    };
    let answered = nsm_process_request(device, asked);
    nsm_exit(device);

    match answered {
        Response::Attestation { document } => Ok(Attested { wrapping, document }),
        Response::Error(code) => Err(AttestError::Nsm(format!("{code:?}"))),
        other => Err(AttestError::Nsm(format!("it answered {other:?}"))),
    }
}
