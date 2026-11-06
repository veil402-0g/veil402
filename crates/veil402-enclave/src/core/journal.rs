//! The enclave's own record of a session, kept by a host that cannot read it.

use alloy::primitives::B256;
use borsh::{BorshDeserialize, BorshSerialize};
use veil402_sdk::session::{EnclaveKey, Sealed};

use crate::Error;

/// One call, as the enclave recorded it.
///
/// Borsh because this round trips byte for byte and a trailing byte is a
/// corrupt record rather than something to ignore.
#[derive(BorshDeserialize, BorshSerialize)]
struct Entry {
    sequence: u64,
    attempt: String,
    total: u64,
    attestation: [u8; 32],
}

/// A call the enclave served, recovered from its own record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recorded {
    /// What named the call, so a retry that straddles a restart is still
    /// answered rather than billed twice.
    pub attempt: String,
    /// The running total once this call was counted.
    pub total: u64,
    pub attestation: B256,
}

/// Seals one record.
///
/// # Errors
///
/// Returns [`Error::Veil`] when the record cannot be encoded or sealed.
pub fn record(
    key: &EnclaveKey,
    sequence: u64,
    attempt: &str,
    total: u64,
    attestation: B256,
) -> Result<Vec<u8>, Error> {
    let entry = borsh::to_vec(&Entry {
        sequence,
        attempt: attempt.to_owned(),
        total,
        attestation: attestation.0,
    })
    .map_err(|_| veil402_sdk::Error::Handover("a record does not encode"))?;
    let sealed = key.seal_to_self(&entry)?;
    let mut bytes = Vec::with_capacity(32 + sealed.ciphertext.len());
    bytes.extend_from_slice(&sealed.encapsulated);
    bytes.extend_from_slice(&sealed.ciphertext);
    Ok(bytes)
}

/// Replays a log this key sealed.
///
/// Refuses anything it did not write: a record that will not open was not
/// sealed by this enclave, and a sequence with a gap is a log the host has
/// edited. Neither is something to resume from.
///
/// A missing tail cannot be detected — the enclave keeps no count of its own —
/// but dropping records only lowers the total, and a client that retries the
/// call it lost has it counted again.
///
/// # Errors
///
/// Returns [`Error::Refused`] when a record does not open or the sequence is
/// not contiguous from zero.
pub fn replay(key: &EnclaveKey, log: &[Vec<u8>]) -> Result<Vec<Recorded>, Error> {
    let mut replayed = Vec::with_capacity(log.len());
    for (position, bytes) in log.iter().enumerate() {
        let (head, ciphertext) = bytes
            .split_at_checked(32)
            .ok_or(Error::Refused("a record is truncated"))?;
        let mut encapsulated = [0_u8; 32];
        encapsulated.copy_from_slice(head);
        let opened = key.open_own(&Sealed {
            encapsulated,
            ciphertext: ciphertext.to_vec(),
        })?;
        let entry = Entry::try_from_slice(&opened)
            .map_err(|_| Error::Refused("a record does not decode"))?;
        if entry.sequence != position as u64 {
            return Err(Error::Refused("the log skips a record"));
        }
        replayed.push(Recorded {
            attempt: entry.attempt,
            total: entry.total,
            attestation: B256::from(entry.attestation),
        });
    }
    Ok(replayed)
}
