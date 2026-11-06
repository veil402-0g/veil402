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

/// A separate scheme for what an enclave seals to itself, so a delegation can
/// never be opened as its own record, nor a record as a delegation.
const JOURNAL: &[u8] = b"veil402:journal:v1";

/// What a buyer sends an enclave to work on. A separate scheme again: a
/// prompt must never open as a delegation, nor a delegation as a prompt.
const PROMPT: &[u8] = b"veil402:prompt:v1";

/// What comes back. Sealed to a key the buyer minted for this one request, so
/// the host between them relays an answer it cannot read.
const REPLY: &[u8] = b"veil402:reply:v1";

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

    /// Seals state to this key, for a host to keep and hand back.
    ///
    /// The enclave holds nothing durable, so its own record of a session has
    /// to live somewhere it does not trust. HPKE rather than a symmetric
    /// scheme because each seal takes a fresh ephemeral key: there is no
    /// nonce to manage, and so none to reuse.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when encryption fails.
    pub fn seal_to_self(&self, payload: &[u8]) -> Result<Sealed, Error> {
        let (encapsulated, ciphertext) = single_shot_seal::<ChaCha20Poly1305, HkdfSha256, Kem>(
            &OpModeS::Base,
            &self.public,
            JOURNAL,
            payload,
            &[],
        )
        .map_err(|_| Error::Handover("state does not seal"))?;
        let bytes = encapsulated.to_bytes();
        let mut key = [0_u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Sealed {
            encapsulated: key,
            ciphertext,
        })
    }

    /// Opens a request sealed to this enclave.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the request was not sealed to this
    /// key, or has been altered.
    pub fn open_prompt(&self, sealed: &Sealed) -> Result<Vec<u8>, Error> {
        self.unseal(sealed, PROMPT)
    }

    /// Seals an answer to the key the request named.
    ///
    /// Safe without an announcement: that key arrived inside a request only
    /// the delegation's holder could have sealed, so it is already as
    /// authenticated as the request itself.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when encryption fails.
    pub fn reply(reply_to: [u8; 32], payload: &[u8]) -> Result<Sealed, Error> {
        seal(reply_to, REPLY, payload)
    }

    /// Opens state this key sealed.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the record was not sealed by this
    /// key, or has been altered.
    pub fn open_own(&self, sealed: &Sealed) -> Result<Vec<u8>, Error> {
        self.unseal(sealed, JOURNAL)
    }

    /// Opens an envelope sealed to this key.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the envelope was not sealed to this
    /// key, or has been altered.
    pub fn open(&self, sealed: &Sealed) -> Result<Vec<u8>, Error> {
        self.unseal(sealed, INFO)
    }

    fn unseal(&self, sealed: &Sealed, info: &[u8]) -> Result<Vec<u8>, Error> {
        let encapsulated = <Kem as hpke::Kem>::EncappedKey::from_bytes(&sealed.encapsulated)
            .map_err(|_| Error::Handover("encapsulated key is malformed"))?;
        single_shot_open::<ChaCha20Poly1305, HkdfSha256, Kem>(
            &OpModeR::Base,
            &self.private,
            &encapsulated,
            info,
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

    /// Seals a request so only the enclave can read it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when encryption fails.
    pub fn seal_prompt(&self, payload: &[u8]) -> Result<Sealed, Error> {
        encrypt(&self.public, PROMPT, payload)
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
        encrypt(&self.public, INFO, payload)
    }
}

/// A one-request channel a buyer opens to an enclave.
///
/// Minted per request and thrown away, so the answer to one call cannot be
/// read with the key from another.
pub struct Exchange {
    key: EnclaveKey,
}

impl Exchange {
    /// Mints the channel. Panics if the system CSPRNG fails, as sealing does.
    #[must_use]
    pub fn open() -> Self {
        let (private, public) = <Kem as hpke::Kem>::gen_keypair();
        Self {
            key: EnclaveKey { private, public },
        }
    }

    /// The half that travels inside the request.
    #[must_use]
    pub fn public(&self) -> [u8; 32] {
        self.key.public()
    }

    /// Opens the answer.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the answer was not sealed to this
    /// channel, or has been altered.
    pub fn receive(&self, sealed: &Sealed) -> Result<Vec<u8>, Error> {
        self.key.unseal(sealed, REPLY)
    }
}

fn seal(public: [u8; 32], info: &[u8], payload: &[u8]) -> Result<Sealed, Error> {
    let parsed = Public::from_bytes(&public).map_err(|_| Error::Handover("key is malformed"))?;
    encrypt(&parsed, info, payload)
}

fn encrypt(public: &Public, info: &[u8], payload: &[u8]) -> Result<Sealed, Error> {
    let (encapsulated, ciphertext) = single_shot_seal::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeS::Base,
        public,
        info,
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
