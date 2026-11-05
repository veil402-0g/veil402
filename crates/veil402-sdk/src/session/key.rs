use alloy::primitives::{Address, B256, Signature};
use hpke::{
    Deserializable, OpModeR, OpModeS, Serializable, aead::ChaCha20Poly1305, kdf::HkdfSha256,
    kem::X25519HkdfSha256, single_shot_open, single_shot_seal,
};

use crate::{client::Error, evm::PoolClient};

type Kem = X25519HkdfSha256;
type Private = <Kem as hpke::Kem>::PrivateKey;
type Public = <Kem as hpke::Kem>::PublicKey;

/// Binds the sealed envelope to the scheme it was written for.
const INFO: &[u8] = b"veil402:handover:v1";

/// An envelope only its recipient can open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sealed {
    /// HPKE encapsulated key.
    pub encapsulated: [u8; 32],
    /// AEAD ciphertext.
    pub ciphertext: Vec<u8>,
}

/// The enclave's side of the channel: an X25519 key for one envelope.
///
/// Derived, never minted, so a restart reaches the same key and envelopes in
/// flight survive it. The public half is vouched for by the secp256k1 key the
/// registry knows, which is what lets a buyer tell a genuine enclave's key
/// from one a seller made up — until attestation replaces the signature with a
/// document binding the same key.
pub struct EnclaveKey {
    private: Private,
    public: Public,
}

impl EnclaveKey {
    /// Derives the keypair RFC 9180 assigns to this input keying material.
    pub(crate) fn from_ikm(ikm: &[u8; 32]) -> Self {
        let (private, public) = <Kem as hpke::Kem>::derive_keypair(ikm);
        Self { private, public }
    }

    /// The half that goes in the 402 challenge.
    #[must_use]
    pub fn public(&self) -> [u8; 32] {
        let bytes = self.public.to_bytes();
        let mut public = [0_u8; 32];
        public.copy_from_slice(&bytes);
        public
    }

    /// The digest to sign with the registered enclave key.
    #[must_use]
    pub fn announcement(&self, pool: &PoolClient) -> B256 {
        pool.enclave_key_digest(self.public())
    }

    /// Opens an envelope sealed to this key.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the envelope was not sealed to this
    /// key, or has been altered.
    pub fn open(&self, sealed: &Sealed) -> Result<Vec<u8>, Error> {
        let encapsulated = <Kem as hpke::Kem>::EncappedKey::from_bytes(&sealed.encapsulated)
            .map_err(|_| Error::Handover("encapsulated key is malformed"))?;
        single_shot_open::<ChaCha20Poly1305, HkdfSha256, Kem>(
            &OpModeR::Base,
            &self.private,
            &encapsulated,
            INFO,
            &sealed.ciphertext,
            &[],
        )
        .map_err(|_| Error::Handover("envelope does not open"))
    }
}

/// A verified enclave key, and the only way to seal to one.
///
/// Constructing it requires a signature that recovers to the enclave the
/// challenge named, so there is no path that seals a session's authority to an
/// unvouched key.
pub struct Recipient {
    public: Public,
}

impl Recipient {
    /// Accepts an announced key, or refuses it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the signature does not recover to
    /// `enclave`, or the key is malformed.
    pub fn accept(
        public: [u8; 32],
        pool: &PoolClient,
        enclave: Address,
        signature: &[u8],
    ) -> Result<Self, Error> {
        let parsed = Signature::try_from(signature)
            .map_err(|_| Error::Handover("announcement signature is malformed"))?;
        let signer = parsed
            .recover_address_from_prehash(&pool.enclave_key_digest(public))
            .map_err(|_| Error::Handover("announcement signature does not recover"))?;
        if signer != enclave {
            return Err(Error::Handover("announced key is not this enclave's"));
        }
        Ok(Self {
            public: Public::from_bytes(&public)
                .map_err(|_| Error::Handover("announced key is malformed"))?,
        })
    }

    /// Seals a payload so only the enclave can read it.
    ///
    /// Panics if the system CSPRNG fails: HPKE needs fresh entropy per seal,
    /// and an unseeded RNG would leak the delegation it is protecting.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when encryption fails.
    pub fn seal(&self, payload: &[u8]) -> Result<Sealed, Error> {
        let (encapsulated, ciphertext) = single_shot_seal::<ChaCha20Poly1305, HkdfSha256, Kem>(
            &OpModeS::Base,
            &self.public,
            INFO,
            payload,
            &[],
        )
        .map_err(|_| Error::Handover("payload does not seal"))?;
        let bytes = encapsulated.to_bytes();
        let mut key = [0_u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Sealed {
            encapsulated: key,
            ciphertext,
        })
    }
}
