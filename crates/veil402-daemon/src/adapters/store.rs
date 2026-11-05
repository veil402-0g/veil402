use async_trait::async_trait;
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use veil402_sdk::{Field, StoredNode};

use crate::core::index::{Batch, Commitment, IndexStore, StoreError};

/// The index, held across restarts.
///
/// The chain only answers a fixed window, so this is the authority for
/// anything older than it and losing the file cannot be recovered by resyncing.
pub struct SqliteIndex {
    pool: SqlitePool,
}

impl SqliteIndex {
    /// Opens the index, creating it if absent.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Unavailable`] when the file cannot be opened or
    /// the schema cannot be applied.
    pub async fn open(url: &str) -> Result<Self, StoreError> {
        let pool = SqlitePoolOptions::new()
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
             INSERT OR IGNORE INTO cursor (id, block) VALUES (1, 0);",
        )
        .execute(&pool)
        .await
        .map_err(|_| StoreError::Unavailable)?;

        Ok(Self { pool })
    }
}

fn field(bytes: Vec<u8>) -> Result<Field, StoreError> {
    let encoded: [u8; 32] = bytes.try_into().map_err(|_| StoreError::Corrupt)?;
    Field::from_bytes(encoded).map_err(|_| StoreError::Corrupt)
}

fn block(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Corrupt)
}

fn commitment(row: &sqlx::sqlite::SqliteRow) -> Result<Commitment, StoreError> {
    Ok(Commitment {
        leaf_index: u32::try_from(row.get::<i64, _>("leaf_index"))
            .map_err(|_| StoreError::Corrupt)?,
        commitment: field(row.get("commitment"))?,
        note: row.get("note"),
        block: u64::try_from(row.get::<i64, _>("block")).map_err(|_| StoreError::Corrupt)?,
    })
}

#[async_trait]
impl IndexStore for SqliteIndex {
    async fn cursor(&self) -> Result<u64, StoreError> {
        let row = sqlx::query("SELECT block FROM cursor WHERE id = 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        u64::try_from(row.get::<i64, _>("block")).map_err(|_| StoreError::Corrupt)
    }

    async fn nodes(&self) -> Result<Vec<StoredNode>, StoreError> {
        let rows = sqlx::query("SELECT level, idx, node FROM nodes ORDER BY level, idx")
            .fetch_all(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;

        rows.into_iter()
            .map(|row| {
                Ok(StoredNode {
                    level: u8::try_from(row.get::<i64, _>("level"))
                        .map_err(|_| StoreError::Corrupt)?,
                    index: u32::try_from(row.get::<i64, _>("idx"))
                        .map_err(|_| StoreError::Corrupt)?,
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

        for held in &batch.commitments {
            sqlx::query(
                "INSERT INTO commitments (leaf_index, commitment, note, block)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(i64::from(held.leaf_index))
            .bind(held.commitment.to_bytes().to_vec())
            .bind(held.note.clone())
            .bind(block(held.block)?)
            .execute(&mut *transaction)
            .await
            .map_err(|_| StoreError::Corrupt)?;
        }
        for spent in &batch.nullifiers {
            sqlx::query("INSERT OR IGNORE INTO nullifiers (nullifier, block) VALUES (?, ?)")
                .bind(spent.nullifier.to_bytes().to_vec())
                .bind(block(spent.block)?)
                .execute(&mut *transaction)
                .await
                .map_err(|_| StoreError::Unavailable)?;
        }
        // The rightmost node of each level is rewritten as the tree grows.
        for held in nodes {
            sqlx::query(
                "INSERT INTO nodes (level, idx, node) VALUES (?, ?, ?)
                 ON CONFLICT (level, idx) DO UPDATE SET node = excluded.node",
            )
            .bind(i64::from(held.level))
            .bind(i64::from(held.index))
            .bind(held.node.to_bytes().to_vec())
            .execute(&mut *transaction)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        }
        sqlx::query("UPDATE cursor SET block = ? WHERE id = 1")
            .bind(block(batch.through)?)
            .execute(&mut *transaction)
            .await
            .map_err(|_| StoreError::Unavailable)?;

        transaction
            .commit()
            .await
            .map_err(|_| StoreError::Unavailable)
    }

    async fn leaf_of(&self, commitment: Field) -> Result<Option<u32>, StoreError> {
        let row = sqlx::query("SELECT leaf_index FROM commitments WHERE commitment = ?")
            .bind(commitment.to_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;

        row.map(|row| u32::try_from(row.get::<i64, _>("leaf_index")))
            .transpose()
            .map_err(|_| StoreError::Corrupt)
    }

    async fn is_spent(&self, nullifier: Field) -> Result<bool, StoreError> {
        let row = sqlx::query("SELECT 1 FROM nullifiers WHERE nullifier = ?")
            .bind(nullifier.to_bytes().to_vec())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(row.is_some())
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
