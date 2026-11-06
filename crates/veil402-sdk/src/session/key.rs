use alloy::primitives::{Address, B256, Signature};
use hpke::{
    Deserializable, OpModeR, OpModeS, Serializable,
    aead::{AeadCtxR, AeadCtxS, ChaCha20Poly1305},
    kdf::HkdfSha256,
    kem::X25519HkdfSha256,
    setup_receiver, setup_sender, single_shot_open, single_shot_seal,
};

use crate::{client::Error, evm::PoolClient};

type Kem = X25519HkdfSha256;
type Private = <Kem as hpke::Kem>::PrivateKey;
type Public = <Kem as hpke::Kem>::PublicKey;
type Sealing = AeadCtxS<ChaCha20Poly1305, HkdfSha256, Kem>;
type Opening = AeadCtxR<ChaCha20Poly1305, HkdfSha256, Kem>;

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

    /// Opens a sealed stream back to the key a request named.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the key is malformed.
    pub fn reply_stream(reply_to: [u8; 32]) -> Result<Outbound, Error> {
        Outbound::to(reply_to, REPLY)
    }

    fn opening(&self, head: [u8; 32], info: &[u8]) -> Result<Inbound, Error> {
        let encapsulated = <Kem as hpke::Kem>::EncappedKey::from_bytes(&head)
            .map_err(|_| Error::Handover("encapsulated key is malformed"))?;
        let context = setup_receiver::<ChaCha20Poly1305, HkdfSha256, Kem>(
            &OpModeR::Base,
            &self.private,
            &encapsulated,
            info,
        )
        .map_err(|_| Error::Handover("stream does not open"))?;
        Ok(Inbound {
            context,
            done: false,
        })
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

    /// Opens the stream an answer arrives on.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the head is malformed.
    pub fn inbound(&self, head: [u8; 32]) -> Result<Inbound, Error> {
        self.key.opening(head, REPLY)
    }
}

/// One frame of a sealed stream, and whether it closed it.
pub struct Frame {
    pub payload: Vec<u8>,
    pub last: bool,
}

/// The sending half of a sealed stream: one context, many frames.
///
/// RFC 9180 gives each frame the next nonce in sequence, so one that is
/// reordered, repeated or dropped will not open. What it does not give is an
/// end — a relay that simply stops forwarding leaves a prefix that opens
/// perfectly well.
///
/// So the sender says which frame is the last, inside the frame. TLS closes
/// the same hole the same way: an authenticated `close_notify`, never a
/// plaintext one, because an end a relay can utter is an end it can forge.
pub struct Outbound {
    context: Sealing,
    encapsulated: [u8; 32],
}

impl Outbound {
    /// Opens a stream to `recipient`. `info` names the scheme, as it does for
    /// every other envelope here — ours, or an upstream's.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the key is malformed or no context
    /// can be established.
    pub fn to(recipient: [u8; 32], info: &[u8]) -> Result<Self, Error> {
        let public =
            Public::from_bytes(&recipient).map_err(|_| Error::Handover("key is malformed"))?;
        let (encapsulated, context) =
            setup_sender::<ChaCha20Poly1305, HkdfSha256, Kem>(&OpModeS::Base, &public, info)
                .map_err(|_| Error::Handover("stream does not open"))?;
        let bytes = encapsulated.to_bytes();
        let mut head = [0_u8; 32];
        head.copy_from_slice(&bytes);
        Ok(Self {
            context,
            encapsulated: head,
        })
    }

    /// The encapsulated key, which travels once ahead of the frames.
    #[must_use]
    pub const fn head(&self) -> [u8; 32] {
        self.encapsulated
    }

    /// Seals one frame.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the frame does not seal, which for a
    /// stream means its sequence is spent.
    pub fn frame(&mut self, payload: &[u8], last: bool) -> Result<Vec<u8>, Error> {
        let mut sealed = Vec::with_capacity(payload.len() + 1);
        sealed.push(u8::from(last));
        sealed.extend_from_slice(payload);
        self.context
            .seal(&sealed, &[])
            .map_err(|_| Error::Handover("frame does not seal"))
    }
}

/// The receiving half. Frames open in the order they were sealed, or not at
/// all.
pub struct Inbound {
    context: Opening,
    done: bool,
}

impl Inbound {
    /// Opens one frame. Whether it was the last is the sender's to say and
    /// nobody else's: it travels inside the seal.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the frame is out of order, altered,
    /// or arrives after the stream has already ended.
    pub fn frame(&mut self, sealed: &[u8]) -> Result<Frame, Error> {
        if self.done {
            return Err(Error::Handover("the stream has already ended"));
        }
        let opened = self
            .context
            .open(sealed, &[])
            .map_err(|_| Error::Handover("frame does not open"))?;
        let (marker, payload) = opened
            .split_first()
            .ok_or(Error::Handover("a frame with no marker"))?;
        self.done = marker != &0;
        Ok(Frame {
            payload: payload.to_vec(),
            last: self.done,
        })
    }

    /// Whether the stream reached an end that was sealed as one. A stream
    /// that stops without it was cut short, however well every frame opened.
    #[must_use]
    pub const fn done(&self) -> bool {
        self.done
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
