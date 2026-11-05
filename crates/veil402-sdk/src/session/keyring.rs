use alloy::{primitives::B256, signers::local::PrivateKeySigner};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use super::key::EnclaveKey;
use crate::{Field, client::Error};

/// Domain tags. Changing one orphans every key previously derived under it.
const HANDOVER: &[u8] = b"veil402:handover-key:v1";
const SIGNER: &[u8] = b"veil402:signer:v1";
const COMMITMENT: &[u8] = b"veil402:seed-commitment:v1";

/// The secret every enclave key descends from.
///
/// Held only in enclave memory. On real hardware it is re-derived at boot from
/// a KMS key gated on attestation, never read from storage, which is what lets
/// a restarted enclave open envelopes sealed before it died.
pub struct RootSeed(Zeroizing<[u8; 32]>);

impl RootSeed {
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Generates a seed from the operating system CSPRNG, for tests and for a
    /// host with no KMS.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Random`] when secure randomness is unavailable.
    pub fn random() -> Result<Self, Error> {
        let mut bytes = [0_u8; 32];
        rand::RngCore::try_fill_bytes(&mut rand::rngs::OsRng, &mut bytes)
            .map_err(|_| Error::Random)?;
        Ok(Self(Zeroizing::new(bytes)))
    }
}

impl std::fmt::Debug for RootSeed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RootSeed(<redacted>)")
    }
}

/// Every key one enclave uses, derived rather than stored.
#[derive(Debug)]
pub struct Keyring {
    root: RootSeed,
}

impl Keyring {
    #[must_use]
    pub const fn new(root: RootSeed) -> Self {
        Self { root }
    }

    /// The X25519 key one envelope's delegation is sealed to.
    ///
    /// Per envelope rather than per enclave: a key that leaked would otherwise
    /// open every session ever delegated here. Per boot would do that too, at
    /// the cost of losing every envelope in flight on a restart.
    #[must_use]
    pub fn handover(&self, nullifier: Field) -> EnclaveKey {
        EnclaveKey::from_ikm(&self.expand(HANDOVER, &nullifier.to_bytes()))
    }

    /// The secp256k1 key the registry knows.
    ///
    /// One per enclave and stable forever: an address that moved would strand
    /// every escrow the old one agreed to meter.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Derive`] if no counter yields a valid scalar, which
    /// takes a break in SHA-256 rather than bad luck.
    pub fn signer(&self) -> Result<PrivateKeySigner, Error> {
        (0..=u8::MAX)
            .find_map(|counter| PrivateKeySigner::from_slice(&self.expand(SIGNER, &[counter])).ok())
            .ok_or(Error::Derive("no counter derives a signing key"))
    }

    /// An opaque identifier for the seed behind this ring.
    ///
    /// Compiled into the image so a boot that derives a different seed — a
    /// re-pointed KMS alias, a changed KDF — refuses to serve rather than
    /// quietly publishing addresses nobody funded.
    #[must_use]
    pub fn commitment(&self) -> B256 {
        B256::from(self.expand(COMMITMENT, &[]))
    }

    fn expand(&self, tag: &[u8], extra: &[u8]) -> [u8; 32] {
        let mut info = Vec::with_capacity(8 + tag.len() + extra.len());
        push(&mut info, tag);
        push(&mut info, extra);
        let mut output = [0_u8; 32];
        Hkdf::<Sha256>::new(None, &self.root.0[..])
            .expand(&info, &mut output)
            .expect("32 bytes is within HKDF-SHA256's output limit");
        output
    }
}

/// Length-prefixes a field so a concatenation of them is injective: `("ab","c")`
/// and `("a","bc")` must not derive the same key.
fn push(buffer: &mut Vec<u8>, field: &[u8]) {
    let length = u32::try_from(field.len()).unwrap_or(u32::MAX);
    buffer.extend_from_slice(&length.to_be_bytes());
    buffer.extend_from_slice(field);
}
