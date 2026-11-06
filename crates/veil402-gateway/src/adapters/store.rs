use std::path::Path;

use async_trait::async_trait;
use borsh::{BorshDeserialize, BorshSerialize};
use redb::{Database, ReadableTable, TableDefinition};
use veil402_sdk::{Field, evm::Session, session::Sealed};

use crate::core::{Kept, MeterError, Resumes};

/// The delegation a session was opened with. Written once.
const OPENED: TableDefinition<[u8; 32], Vec<u8>> = TableDefinition::new("opened");

/// One sealed record per call, keyed by position so a write is a write and
/// not a rewrite of everything before it.
const LOG: TableDefinition<([u8; 32], u64), Vec<u8>> = TableDefinition::new("log");

#[derive(BorshDeserialize, BorshSerialize)]
struct Opening {
    encapsulated: [u8; 32],
    ciphertext: Vec<u8>,
    nullifier: [u8; 32],
    seller: [u8; 20],
    deadline: u64,
}

fn unavailable(error: impl std::fmt::Display) -> MeterError {
    MeterError::Refused(format!("the session store is unavailable: {error}"))
}

/// What the host keeps so a restarted enclave can pick its sessions back up.
pub struct Sessions {
    database: Database,
}

impl Sessions {
    /// # Errors
    ///
    /// Returns [`MeterError::Refused`] when the file cannot be opened.
    pub fn open(path: &Path) -> Result<Self, MeterError> {
        let database = Database::create(path).map_err(unavailable)?;
        // Both tables up front: a read transaction refuses one that has never
        // been written, and "nothing kept yet" is the normal case at boot.
        let write = database.begin_write().map_err(unavailable)?;
        write.open_table(OPENED).map_err(unavailable)?;
        write.open_table(LOG).map_err(unavailable)?;
        write.commit().map_err(unavailable)?;
        Ok(Self { database })
    }
}

#[async_trait]
impl Resumes for Sessions {
    async fn opened(
        &self,
        nullifier: Field,
        sealed: &Sealed,
        session: Session,
    ) -> Result<(), MeterError> {
        let encoded = borsh::to_vec(&Opening {
            encapsulated: sealed.encapsulated,
            ciphertext: sealed.ciphertext.clone(),
            nullifier: session.nullifier.to_bytes(),
            seller: session.seller.into_array(),
            deadline: session.deadline,
        })
        .map_err(unavailable)?;
        let write = self.database.begin_write().map_err(unavailable)?;
        write
            .open_table(OPENED)
            .map_err(unavailable)?
            .insert(nullifier.to_bytes(), encoded)
            .map_err(unavailable)?;
        write.commit().map_err(unavailable)
    }

    async fn append(&self, nullifier: Field, record: &[u8]) -> Result<(), MeterError> {
        let write = self.database.begin_write().map_err(unavailable)?;
        {
            let mut log = write.open_table(LOG).map_err(unavailable)?;
            let next = log
                .range((nullifier.to_bytes(), 0)..=(nullifier.to_bytes(), u64::MAX))
                .map_err(unavailable)?
                .count() as u64;
            log.insert((nullifier.to_bytes(), next), record.to_vec())
                .map_err(unavailable)?;
        }
        write.commit().map_err(unavailable)
    }

    async fn recall(&self, nullifier: Field) -> Result<Option<Kept>, MeterError> {
        let read = self.database.begin_read().map_err(unavailable)?;
        let opened = read.open_table(OPENED).map_err(unavailable)?;
        let Some(encoded) = opened.get(nullifier.to_bytes()).map_err(unavailable)? else {
            return Ok(None);
        };
        let opening = Opening::try_from_slice(&encoded.value()).map_err(unavailable)?;
        let log = read.open_table(LOG).map_err(unavailable)?;
        let records = log
            .range((nullifier.to_bytes(), 0)..=(nullifier.to_bytes(), u64::MAX))
            .map_err(unavailable)?
            .map(|row| row.map(|(_, record)| record.value()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(unavailable)?;

        Ok(Some(Kept {
            sealed: Sealed {
                encapsulated: opening.encapsulated,
                ciphertext: opening.ciphertext,
            },
            session: Session {
                nullifier: Field::from_bytes(opening.nullifier).map_err(unavailable)?,
                seller: opening.seller.into(),
                deadline: opening.deadline,
            },
            log: records,
        }))
    }

    async fn forget(&self, nullifier: Field) -> Result<(), MeterError> {
        let write = self.database.begin_write().map_err(unavailable)?;
        {
            write
                .open_table(OPENED)
                .map_err(unavailable)?
                .remove(nullifier.to_bytes())
                .map_err(unavailable)?;
            let mut log = write.open_table(LOG).map_err(unavailable)?;
            log.retain_in(
                (nullifier.to_bytes(), 0)..=(nullifier.to_bytes(), u64::MAX),
                |_, _| false,
            )
            .map_err(unavailable)?;
        }
        write.commit().map_err(unavailable)
    }
}
