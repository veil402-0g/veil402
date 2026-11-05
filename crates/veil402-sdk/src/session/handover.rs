use alloy::primitives::Address;
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    client::Error,
    evm::Session,
    protocol::{Field, Note, SessionKeys, Secret, TREE_DEPTH, note},
};

/// Layout version. An enclave and a daemon are run by different parties and
/// upgrade independently, so an unrecognised version is refused rather than
/// partially understood.
const VERSION: u8 = 1;

/// The wire form. Borsh is canonical by construction and rejects trailing
/// bytes, so the encoding needs no framing of its own and no offset
/// arithmetic to get wrong.
#[derive(BorshSerialize, BorshDeserialize)]
struct Wire {
    version: u8,
    seed: [u8; 32],
    value: u64,
    asset: [u8; 32],
    leaf: u32,
    siblings: [[u8; 32]; TREE_DEPTH],
    root: [u8; 32],
    change_owner: [u8; 32],
    target: u64,
    seller: [u8; 20],
    deadline: u64,
    pool: [u8; 20],
    chain_id: u64,
    node: [u8; 20],
    tee_signer: [u8; 20],
}

/// What a buyer hands a delegate so it can meter a session and settle it.
///
/// The seed is the whole delegation: it opens one note and reaches nothing
/// else. The path never expires because the pool keeps every root it has
/// reached, so the delegate needs no indexer and no RPC. Pool, chain and TEE
/// signer travel here rather than being compiled in — the measurement commits
/// to the rule the delegate follows, not to the data it follows it against.
#[derive(Debug)]
pub struct Handover {
    /// Opens the session note, and only that note.
    pub seed: Secret,
    pub value: u64,
    pub asset: Field,
    pub leaf: u32,
    pub siblings: [Field; TREE_DEPTH],
    /// What those siblings must reproduce.
    pub root: Field,
    /// Where change goes.
    pub change_owner: Field,
    /// Size a settlement should leave behind as a second note.
    pub target: u64,
    pub seller: Address,
    pub deadline: u64,
    pub pool: Address,
    pub chain_id: u64,
    /// The 0G node this session will be served by.
    pub node: Address,
    /// Its signer, read from the Serving contract by the buyer — the party a
    /// wrong one would harm.
    pub tee_signer: Address,
}

impl Handover {
    /// Encodes the payload for sealing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the payload cannot be encoded.
    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        borsh::to_vec(&Wire {
            version: VERSION,
            seed: self.seed.to_bytes(),
            value: self.value,
            asset: self.asset.to_bytes(),
            leaf: self.leaf,
            siblings: self.siblings.map(Field::to_bytes),
            root: self.root.to_bytes(),
            change_owner: self.change_owner.to_bytes(),
            target: self.target,
            seller: self.seller.into(),
            deadline: self.deadline,
            pool: self.pool.into(),
            chain_id: self.chain_id,
            node: self.node.into(),
            tee_signer: self.tee_signer.into(),
        })
        .map_err(|_| Error::Handover("handover does not encode"))
    }

    /// Decodes a payload a delegate has just opened.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the version is unknown, the encoding is
    /// malformed or trailing, or a field is not a canonical `BN254` element.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let wire = Wire::try_from_slice(bytes)
            .map_err(|_| Error::Handover("handover does not decode"))?;
        if wire.version != VERSION {
            return Err(Error::Handover("handover version is not supported"));
        }
        let mut siblings = [Field::ZERO; TREE_DEPTH];
        for (sibling, bytes) in siblings.iter_mut().zip(wire.siblings) {
            *sibling = Field::from_bytes(bytes)?;
        }
        Ok(Self {
            seed: Secret::from_bytes(wire.seed)?,
            value: wire.value,
            asset: Field::from_bytes(wire.asset)?,
            leaf: wire.leaf,
            siblings,
            root: Field::from_bytes(wire.root)?,
            change_owner: Field::from_bytes(wire.change_owner)?,
            target: wire.target,
            seller: Address::from(wire.seller),
            deadline: wire.deadline,
            pool: Address::from(wire.pool),
            chain_id: wire.chain_id,
            node: Address::from(wire.node),
            tee_signer: Address::from(wire.tee_signer),
        })
    }

    /// Checks the handover against the escrow the delegate agreed to meter,
    /// and returns the keys it opens.
    ///
    /// Everything here is arithmetic, so a delegate with no chain access can
    /// run it. What it cannot check is that `root` is one the pool has
    /// reached; whoever broadcasts the settlement has an RPC and checks
    /// `knownRoot` before serving.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the seed does not open the escrowed
    /// note, the path does not reproduce `root`, or the seller and deadline
    /// disagree with the escrow.
    pub fn accept(&self, session: &Session) -> Result<SessionKeys, Error> {
        if self.seller != session.seller || self.deadline != session.deadline {
            return Err(Error::Handover("handover is for a different session"));
        }
        let keys = SessionKeys::from_seed(&self.seed)?;
        if keys.owner.nullifier(self.leaf)? != session.nullifier {
            return Err(Error::Handover("seed does not open the escrowed note"));
        }
        let note = Note {
            value: self.value,
            asset: self.asset,
            random: SessionKeys::from_seed(&self.seed)?.random,
        };
        if note::merkle_root(note.commitment(&keys.owner)?, self.leaf, &self.siblings)? != self.root
        {
            return Err(Error::Handover("path does not reach the stated root"));
        }
        Ok(keys)
    }
}
