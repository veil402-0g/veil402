//! Where the enclave's root seed comes from, and the gate it has to pass.
//!
//! Nothing is stored. The seed is agreed with KMS against a fixed peer point,
//! which makes it the same on every boot and on every machine — so there is
//! no secret at rest to steal, and losing the host loses nothing.
//!
//! The last step is a commitment check against a value compiled into the
//! image. A re-pointed key, a changed peer point or a drifted KDF would all
//! hand back a *different* seed rather than no seed, and the enclave would
//! come up serving addresses nobody funded. This turns that into a refusal.

use hkdf::Hkdf;
use openssl::{cms::CmsContentInfo, pkey::PKey};
use rsa::{RsaPrivateKey, pkcs8::EncodePrivateKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::{
    kms::{Kms, KmsError},
    nsm::{self, AttestError},
    nums::{self, NumsError},
};

/// Fixes what this KDF is for. Changing it rotates every key the enclave has.
const SEED_INFO: &[u8] = b"veil402/seed/v1";
const COMMITMENT_INFO: &[u8] = b"veil402/seed-commitment/v1";

#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    #[error(transparent)]
    Attest(#[from] AttestError),
    #[error(transparent)]
    Kms(#[from] KmsError),
    #[error(transparent)]
    Nums(#[from] NumsError),
    #[error("the wrapped secret will not open: {0}")]
    Unwrap(String),
    #[error("the key derivation refused: {0}")]
    Kdf(String),
    /// The seed is not the one this image was built for. Refusing is the
    /// point: carrying on would publish an identity nobody is watching.
    #[error("this image expects the seed committing to {expected}, and derived {got}")]
    WrongSeed { expected: String, got: String },
}

/// The enclave's root secret, zeroized when it goes.
pub struct RootSeed(Zeroizing<[u8; 32]>);

impl RootSeed {
    #[must_use]
    pub fn expose(&self) -> [u8; 32] {
        *self.0
    }

    /// What this seed is, said in a way that reveals nothing about it.
    #[must_use]
    pub fn commitment(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(COMMITMENT_INFO);
        hash.update(self.0.as_ref());
        hash.finalize().into()
    }
}

/// Attests, agrees, unwraps, derives, and refuses anything but the expected
/// seed.
///
/// `expected` empty is bootstrap: the commitment is reported rather than
/// enforced, which is how the first image learns the value to be built with.
/// Shipping it empty leaves the gate open, so it is a one-time state.
///
/// # Errors
///
/// Returns [`SeedError`] at whichever step refuses. A [`KmsError::Denied`]
/// inside it means the measurements did not satisfy the key policy.
pub async fn load(kms: &Kms, key: &str, expected: &str) -> Result<RootSeed, SeedError> {
    let attested = nsm::attest()?;
    let wrapped = kms.agree(key, &nums::peer_spki()?, &attested.document).await?;
    let secret = unwrap(&wrapped, &attested.wrapping)?;
    let seed = derive(&secret)?;
    gate(&seed, expected)?;
    Ok(seed)
}

/// Opens the CMS envelope KMS wrapped the secret in.
fn unwrap(envelope: &[u8], wrapping: &RsaPrivateKey) -> Result<Zeroizing<Vec<u8>>, SeedError> {
    let pkcs8 = wrapping
        .to_pkcs8_der()
        .map_err(|error| SeedError::Unwrap(error.to_string()))?;
    let key = PKey::private_key_from_pkcs8(pkcs8.as_bytes())
        .map_err(|error| SeedError::Unwrap(error.to_string()))?;
    // KMS emits BER with indefinite lengths, which a strict DER parser
    // rejects; OpenSSL takes it.
    let envelope = CmsContentInfo::from_der(envelope)
        .map_err(|error| SeedError::Unwrap(error.to_string()))?;
    // No recipient certificate exists — KMS names the recipient by key
    // identifier and only the bare key is held. The wrap is RSAES-OAEP, so
    // the usual caveat about trial decryption does not apply.
    let plaintext = envelope
        .decrypt_without_cert_check(&key)
        .map_err(|error| SeedError::Unwrap(error.to_string()))?;
    Ok(Zeroizing::new(plaintext))
}

/// Turns the agreed secret into the seed, bound to the point it was agreed
/// against.
fn derive(secret: &[u8]) -> Result<RootSeed, SeedError> {
    let peer = nums::peer_compressed()?;
    let mut info = Vec::with_capacity(8 + SEED_INFO.len() + peer.len());
    for field in [SEED_INFO, peer.as_slice()] {
        info.extend_from_slice(&u32::try_from(field.len()).unwrap_or(u32::MAX).to_be_bytes());
        info.extend_from_slice(field);
    }
    let mut seed = Zeroizing::new([0_u8; 32]);
    Hkdf::<Sha256>::new(None, secret)
        .expand(&info, seed.as_mut())
        .map_err(|error| SeedError::Kdf(error.to_string()))?;
    Ok(RootSeed(seed))
}

fn gate(seed: &RootSeed, expected: &str) -> Result<(), SeedError> {
    let got = hex(&seed.commitment());
    if expected.is_empty() {
        tracing::warn!(
            commitment = %got,
            "no expected seed is compiled in; build the next image with this and the gate closes"
        );
        return Ok(());
    }
    if !expected.eq_ignore_ascii_case(&got) {
        return Err(SeedError::WrongSeed { expected: expected.to_owned(), got });
    }
    tracing::info!(commitment = %got, "the seed is the one this image was built for");
    Ok(())
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().fold(String::with_capacity(64), |mut out, byte| {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_of(byte: u8) -> RootSeed {
        RootSeed(Zeroizing::new([byte; 32]))
    }

    /// An empty expectation reports rather than enforces, which is the only
    /// way a first image can learn what to pin.
    #[test]
    fn bootstrap_reports_without_gating() {
        assert!(gate(&seed_of(1), "").is_ok());
    }

    /// The gate is the whole reason a wrong seed is not a quiet success.
    #[test]
    fn a_different_seed_refuses() {
        let seed = seed_of(1);
        assert!(gate(&seed, &hex(&seed.commitment())).is_ok());
        let other = seed_of(2);
        assert!(matches!(
            gate(&other, &hex(&seed.commitment())),
            Err(SeedError::WrongSeed { .. })
        ));
    }

    /// The commitment must say nothing about the seed beyond identifying it.
    #[test]
    fn the_commitment_is_not_the_seed() {
        let seed = seed_of(7);
        assert_ne!(seed.commitment(), seed.expose());
    }
}
