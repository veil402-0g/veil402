use async_trait::async_trait;
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions, sqlite::SqliteRow};
use veil402_sdk::{Field, Secret, StoredNode};

use crate::core::{
    envelope::{EnvelopeStore, SubmissionStore},
    StoreError,
    index::{Batch, Commitment, IndexStore},
    wallet::{ChainFacts, Held, Minted, NoteStore, Unplaced},
};

/// The daemon's store: the chain mirror and the notes we own, in one file.
///
/// The chain only answers a fixed window and note ciphertexts are not
/// decryptable, so neither half can be recovered by resyncing. One file also
/// means one transaction, which is what keeps the tree and its cursor in step.
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Opens the store, creating it if absent.
    ///
    /// One connection: the daemon is a single writer, `SQLite` serialises
    /// writes anyway, and it lets a test run the real schema against
    /// `sqlite::memory:` rather than a mock of it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Unavailable`] when the file cannot be opened or
    /// the schema cannot be applied.
    pub async fn open(url: &str) -> Result<Self, StoreError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(url)
            .await
            .map_err(|_| StoreError::Unavailable)?;

        sqlx::raw_sql(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS commitments (
                 leaf_index INTEGER PRIMARY KEY,
                 commitment BLOB    NOT NULL UNIQUE,
                 note       BLOB    NOT NULL,
                 block      INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS nullifiers (
                 nullifier BLOB    PRIMARY KEY,
                 block     INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS escrows (
                 nullifier BLOB    PRIMARY KEY,
                 seller    BLOB    NOT NULL,
                 enclave   BLOB    NOT NULL,
                 deadline  INTEGER NOT NULL,
                 block     INTEGER NOT NULL,
                 amount    INTEGER
             );
             CREATE TABLE IF NOT EXISTS envelopes (
                 nullifier   BLOB PRIMARY KEY,
                 model       TEXT NOT NULL,
                 change_seed BLOB NOT NULL,
                 opened      INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS submissions (
                 key    BLOB PRIMARY KEY,
                 handle BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS nodes (
                 level INTEGER NOT NULL,
                 idx   INTEGER NOT NULL,
                 node  BLOB    NOT NULL,
                 PRIMARY KEY (level, idx)
             );
             CREATE TABLE IF NOT EXISTS cursor (
                 id    INTEGER PRIMARY KEY CHECK (id = 1),
                 block INTEGER NOT NULL
             );
             INSERT OR IGNORE INTO cursor (id, block) VALUES (1, 0);
             CREATE TABLE IF NOT EXISTS owned (
                 commitment BLOB PRIMARY KEY,
                 leaf_index INTEGER,
                 nullifier  BLOB,
                 value      INTEGER NOT NULL,
                 asset      BLOB    NOT NULL,
                 random     BLOB    NOT NULL,
                 spent      INTEGER NOT NULL DEFAULT 0,
                 held       INTEGER NOT NULL DEFAULT 0,
                 created    INTEGER NOT NULL,
                 seed       BLOB
             );
             CREATE INDEX IF NOT EXISTS owned_nullifier ON owned (nullifier);",
        )
        .execute(&pool)
        .await
        .map_err(|_| StoreError::Unavailable)?;

        Ok(Self { pool })
    }

    /// Shared by `IndexStore` and `ChainFacts`, which ask the same question
    /// for different reasons.
    async fn placed_at(&self, commitment: Field) -> Result<Option<u32>, StoreError> {
        let row = sqlx::query("SELECT leaf_index FROM commitments WHERE commitment = ?")
            .bind(commitment.to_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        row.map(|row| leaf(&row, "leaf_index")).transpose()
    }

    async fn nullified(&self, nullifier: Field) -> Result<bool, StoreError> {
        let row = sqlx::query("SELECT 1 FROM nullifiers WHERE nullifier = ?")
            .bind(nullifier.to_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(row.is_some())
    }

}

/// Every column a placed note needs to be spent again.
const NOTE: &str =
    "SELECT commitment, leaf_index, nullifier, value, asset, random, seed FROM owned";


fn field(bytes: Vec<u8>) -> Result<Field, StoreError> {
    let encoded: [u8; 32] = bytes.try_into().map_err(|_| StoreError::Corrupt)?;
    Field::from_bytes(encoded).map_err(|_| StoreError::Corrupt)
}

fn secret(bytes: Vec<u8>) -> Result<Secret, StoreError> {
    let encoded: [u8; 32] = bytes.try_into().map_err(|_| StoreError::Corrupt)?;
    Secret::from_bytes(encoded).map_err(|_| StoreError::Corrupt)
}

fn integer(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Corrupt)
}

fn count(row: &SqliteRow, column: &str) -> Result<u64, StoreError> {
    u64::try_from(row.get::<i64, _>(column)).map_err(|_| StoreError::Corrupt)
}

fn leaf(row: &SqliteRow, column: &str) -> Result<u32, StoreError> {
    u32::try_from(row.get::<i64, _>(column)).map_err(|_| StoreError::Corrupt)
}

fn commitment(row: &SqliteRow) -> Result<Commitment, StoreError> {
    Ok(Commitment {
        leaf_index: leaf(row, "leaf_index")?,
        commitment: field(row.get("commitment"))?,
        note: row.get("note"),
        block: count(row, "block")?,
    })
}

fn held(row: &SqliteRow) -> Result<Held, StoreError> {
    Ok(Held {
        commitment: field(row.get("commitment"))?,
        leaf: leaf(row, "leaf_index")?,
        nullifier: field(row.get("nullifier"))?,
        value: count(row, "value")?,
        asset: field(row.get("asset"))?,
        random: secret(row.get("random"))?,
        seed: row
            .get::<Option<Vec<u8>>, _>("seed")
            .map(secret)
            .transpose()?,
    })
}

#[async_trait]
impl IndexStore for SqliteStore {
    async fn cursor(&self) -> Result<u64, StoreError> {
        let row = sqlx::query("SELECT block FROM cursor WHERE id = 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        count(&row, "block")
    }

    async fn nodes(&self) -> Result<Vec<StoredNode>, StoreError> {
        let rows = sqlx::query("SELECT level, idx, node FROM nodes ORDER BY level, idx")
            .fetch_all(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;

        rows.iter()
            .map(|row| {
                Ok(StoredNode {
                    level: u8::try_from(row.get::<i64, _>("level"))
                        .map_err(|_| StoreError::Corrupt)?,
                    index: leaf(row, "idx")?,
                    node: field(row.get("node"))?,
                })
            })
            .collect()
    }

    /// One transaction: a crash between the rows and the cursor would replay
    /// leaves at shifted indices on the next start.
    async fn append(&self, batch: &Batch, nodes: &[StoredNode]) -> Result<(), StoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| StoreError::Unavailable)?;

        for note in &batch.commitments {
            sqlx::query(
                "INSERT INTO commitments (leaf_index, commitment, note, block)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(i64::from(note.leaf_index))
            .bind(note.commitment.to_bytes().to_vec())
            .bind(note.note.clone())
            .bind(integer(note.block)?)
            .execute(&mut *transaction)
            .await
            .map_err(|_| StoreError::Corrupt)?;
        }
        for spent in &batch.nullifiers {
            sqlx::query("INSERT OR IGNORE INTO nullifiers (nullifier, block) VALUES (?, ?)")
                .bind(spent.nullifier.to_bytes().to_vec())
                .bind(integer(spent.block)?)
                .execute(&mut *transaction)
                .await
                .map_err(|_| StoreError::Unavailable)?;
        }
        // A nullifier can only be escrowed once; the pool refuses the second.
        for lock in &batch.locks {
            sqlx::query(
                "INSERT OR IGNORE INTO escrows (nullifier, seller, enclave, deadline, block)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(lock.nullifier.to_bytes().to_vec())
            .bind(lock.seller.to_vec())
            .bind(lock.enclave.to_vec())
            .bind(integer(lock.deadline)?)
            .bind(integer(lock.block)?)
            .execute(&mut *transaction)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        }
        // The pool refuses a settlement without an escrow, and locks are
        // applied above, so the row is always there to update.
        for settled in &batch.settlements {
            sqlx::query("UPDATE escrows SET amount = ? WHERE nullifier = ?")
                .bind(integer(settled.amount)?)
                .bind(settled.nullifier.to_bytes().to_vec())
                .execute(&mut *transaction)
                .await
                .map_err(|_| StoreError::Unavailable)?;
        }
        // The rightmost node of each level is rewritten as the tree grows.
        for node in nodes {
            sqlx::query(
                "INSERT INTO nodes (level, idx, node) VALUES (?, ?, ?)
                 ON CONFLICT (level, idx) DO UPDATE SET node = excluded.node",
            )
            .bind(i64::from(node.level))
            .bind(i64::from(node.index))
            .bind(node.node.to_bytes().to_vec())
            .execute(&mut *transaction)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        }
        sqlx::query("UPDATE cursor SET block = ? WHERE id = 1")
            .bind(integer(batch.through)?)
            .execute(&mut *transaction)
            .await
            .map_err(|_| StoreError::Unavailable)?;

        transaction
            .commit()
            .await
            .map_err(|_| StoreError::Unavailable)
    }

    async fn leaf_of(&self, commitment: Field) -> Result<Option<u32>, StoreError> {
        self.placed_at(commitment).await
    }

    async fn is_spent(&self, nullifier: Field) -> Result<bool, StoreError> {
        self.nullified(nullifier).await
    }

    async fn notes(&self, from: u32, limit: u32) -> Result<Vec<Commitment>, StoreError> {
        let rows = sqlx::query(
            "SELECT leaf_index, commitment, note, block FROM commitments
             WHERE leaf_index >= ? ORDER BY leaf_index LIMIT ?",
        )
        .bind(i64::from(from))
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(|_| StoreError::Unavailable)?;

        rows.iter().map(commitment).collect()
    }
}

#[async_trait]
impl ChainFacts for SqliteStore {
    async fn leaf_of(&self, commitment: Field) -> Result<Option<u32>, StoreError> {
        self.placed_at(commitment).await
    }

    async fn is_spent(&self, nullifier: Field) -> Result<bool, StoreError> {
        self.nullified(nullifier).await
    }

    async fn escrow(&self, nullifier: Field) -> Result<Option<u64>, StoreError> {
        let row = sqlx::query("SELECT deadline FROM escrows WHERE nullifier = ?")
            .bind(nullifier.to_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        row.map(|row| count(&row, "deadline")).transpose()
    }

    async fn settlement(&self, nullifier: Field) -> Result<Option<u64>, StoreError> {
        let row = sqlx::query("SELECT amount FROM escrows WHERE nullifier = ? AND amount IS NOT NULL")
            .bind(nullifier.to_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        row.map(|row| count(&row, "amount")).transpose()
    }
}

#[async_trait]
impl EnvelopeStore for SqliteStore {
    /// Written once. A second open under the same nullifier cannot happen —
    /// the pool refuses it — and overwriting would lose the change anyway.
    async fn remember(
        &self,
        nullifier: Field,
        model: &str,
        change_seed: &Secret,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT OR IGNORE INTO envelopes (nullifier, model, change_seed, opened)
             VALUES (?, ?, ?, (SELECT COALESCE(MAX(opened), 0) + 1 FROM envelopes))",
        )
        .bind(nullifier.to_bytes().to_vec())
        .bind(model)
        .bind(change_seed.to_bytes().to_vec())
        .execute(&self.pool)
        .await
        .map_err(|_| StoreError::Unavailable)?;
        Ok(())
    }

    async fn newest(&self, model: &str) -> Result<Option<Field>, StoreError> {
        let row = sqlx::query(
            "SELECT nullifier FROM envelopes WHERE model = ? ORDER BY opened DESC LIMIT 1",
        )
        .bind(model)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| StoreError::Unavailable)?;
        row.map(|row| field(row.get::<Vec<u8>, _>("nullifier")))
            .transpose()
    }

    async fn change_seed(&self, nullifier: Field) -> Result<Option<Secret>, StoreError> {
        let row = sqlx::query("SELECT change_seed FROM envelopes WHERE nullifier = ?")
            .bind(nullifier.to_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        row.map(|row| secret(row.get::<Vec<u8>, _>("change_seed"))).transpose()
    }
}

#[async_trait]
impl SubmissionStore for SqliteStore {
    /// First writer wins, so a racing resubmission adopts the call already in
    /// flight rather than replacing its handle with a second one.
    async fn track(&self, key: Field, handle: &[u8]) -> Result<(), StoreError> {
        sqlx::query("INSERT OR IGNORE INTO submissions (key, handle) VALUES (?, ?)")
            .bind(key.to_bytes().to_vec())
            .bind(handle.to_vec())
            .execute(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(())
    }

    async fn handle(&self, key: Field) -> Result<Option<Vec<u8>>, StoreError> {
        let row = sqlx::query("SELECT handle FROM submissions WHERE key = ?")
            .bind(key.to_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(row.map(|row| row.get::<Vec<u8>, _>("handle")))
    }
}

#[async_trait]
impl NoteStore for SqliteStore {
    /// Re-recording a note is not an error: the same outputs are written again
    /// whenever a send is retried.
    async fn remember(&self, minted: &[Minted], now: u64) -> Result<(), StoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| StoreError::Unavailable)?;

        for note in minted {
            sqlx::query(
                "INSERT OR IGNORE INTO owned (commitment, value, asset, random, created, seed)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(note.commitment.to_bytes().to_vec())
            .bind(integer(note.value)?)
            .bind(note.asset.to_bytes().to_vec())
            .bind(note.random.to_bytes().to_vec())
            .bind(integer(now)?)
            .bind(note.seed.as_ref().map(|seed| seed.to_bytes().to_vec()))
            .execute(&mut *transaction)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        }

        transaction
            .commit()
            .await
            .map_err(|_| StoreError::Unavailable)
    }

    async fn unplaced(&self) -> Result<Vec<Unplaced>, StoreError> {
        let rows = sqlx::query(
            "SELECT commitment, created, seed FROM owned WHERE leaf_index IS NULL AND spent = 0",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| StoreError::Unavailable)?;

        rows.iter()
            .map(|row| {
                Ok(Unplaced {
                    commitment: field(row.get("commitment"))?,
                    created: count(row, "created")?,
                    seed: row
                        .get::<Option<Vec<u8>>, _>("seed")
                        .map(secret)
                        .transpose()?,
                })
            })
            .collect()
    }

    async fn place(&self, commitment: Field, leaf: u32, nullifier: Field) -> Result<(), StoreError> {
        sqlx::query("UPDATE owned SET leaf_index = ?, nullifier = ? WHERE commitment = ?")
            .bind(i64::from(leaf))
            .bind(nullifier.to_bytes().to_vec())
            .bind(commitment.to_bytes().to_vec())
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|_| StoreError::Unavailable)
    }

    async fn outstanding(&self) -> Result<Vec<Held>, StoreError> {
        let rows = sqlx::query(&format!("{NOTE} WHERE nullifier IS NOT NULL AND spent = 0"))
            .fetch_all(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        rows.iter().map(held).collect()
    }

    async fn by_commitment(&self, commitment: Field) -> Result<Option<Held>, StoreError> {
        let row = sqlx::query(&format!("{NOTE} WHERE commitment = ? AND nullifier IS NOT NULL"))
            .bind(commitment.to_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        row.as_ref().map(held).transpose()
    }

    async fn by_nullifier(&self, nullifier: Field) -> Result<Option<Held>, StoreError> {
        let row = sqlx::query(&format!("{NOTE} WHERE nullifier = ?"))
            .bind(nullifier.to_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        row.as_ref().map(held).transpose()
    }

    async fn spend(&self, nullifiers: &[Field]) -> Result<(), StoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| StoreError::Unavailable)?;

        for nullifier in nullifiers {
            sqlx::query("UPDATE owned SET spent = 1 WHERE nullifier = ?")
                .bind(nullifier.to_bytes().to_vec())
                .execute(&mut *transaction)
                .await
                .map_err(|_| StoreError::Unavailable)?;
        }

        transaction
            .commit()
            .await
            .map_err(|_| StoreError::Unavailable)
    }

    async fn spendable(&self, asset: Field, now: u64) -> Result<Vec<Held>, StoreError> {
        let rows = sqlx::query(&format!(
            "{NOTE} WHERE nullifier IS NOT NULL AND spent = 0
             AND asset = ? AND held <= ? ORDER BY value"
        ))
        .bind(asset.to_bytes().to_vec())
        .bind(integer(now)?)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| StoreError::Unavailable)?;
        rows.iter().map(held).collect()
    }

    /// Conditional, so a claim is the decision rather than a read followed by
    /// a write that assumes nothing moved in between.
    async fn hold(&self, commitment: Field, now: u64, until: u64) -> Result<bool, StoreError> {
        let result = sqlx::query("UPDATE owned SET held = ? WHERE commitment = ? AND held <= ?")
            .bind(integer(until)?)
            .bind(commitment.to_bytes().to_vec())
            .bind(integer(now)?)
            .execute(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(result.rows_affected() == 1)
    }

    /// Idempotent: once the hold reaches the escrow's deadline, later passes
    /// change nothing and report nothing.
    async fn extend(&self, commitment: Field, until: u64) -> Result<bool, StoreError> {
        let result = sqlx::query("UPDATE owned SET held = ? WHERE commitment = ? AND held < ?")
            .bind(integer(until)?)
            .bind(commitment.to_bytes().to_vec())
            .bind(integer(until)?)
            .execute(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(result.rows_affected() == 1)
    }

    async fn free(&self, commitment: Field) -> Result<(), StoreError> {
        sqlx::query("UPDATE owned SET held = 0 WHERE commitment = ?")
            .bind(commitment.to_bytes().to_vec())
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|_| StoreError::Unavailable)
    }
}
