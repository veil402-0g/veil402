//! The discovery cursor against a file-backed store.
//!
//! The daemon runs on a file; every other test here runs on `:memory:`, and
//! the sync loop halts outright on a store error, so a query that works in
//! one and not the other takes the whole daemon down.

use std::sync::Arc;

use veil402_daemon::{adapters::SqliteStore, core::wallet::ChainFacts};

type Fallible = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn the_cursor_reads_on_a_file_backed_store() -> Fallible {
    let home = tempfile::tempdir()?;
    let path = home.path().join("veil.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());

    let store = Arc::new(SqliteStore::open(&url).await?);
    assert_eq!(store.scan_mark().await?, 0, "a fresh store starts at leaf 0");
    assert!(store.unopened(0, 64).await?.is_empty());
    store.scanned(9).await?;
    assert_eq!(store.scan_mark().await?, 9);

    // Reopening runs the additive migrations over a store that already has
    // the columns, which is the case every restart hits.
    drop(store);
    let reopened = SqliteStore::open(&url).await?;
    assert_eq!(reopened.scan_mark().await?, 9, "the mark did not survive a restart");
    Ok(())
}
