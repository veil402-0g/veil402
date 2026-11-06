//! Runs the Veil gateway.

use std::error::Error;

use alloy::hex;
use clap::{Parser, Subcommand};
use veil402_daemon::app::{self, Config, Daemon};
use veil402_sdk::Owner;

#[derive(Parser)]
#[command(version, about = "Shielded, mandate-governed payments for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Prints a fresh owner for the environment file.
    Keygen,
    /// Shields tokens into the pool, under this daemon's key.
    Deposit {
        /// Pool units to shield, roughly a micro-USD each.
        amount: u64,
    },
    /// The keys agents authenticate with.
    Key {
        #[command(subcommand)]
        what: Keys,
    },
}

#[derive(Subcommand)]
enum Keys {
    /// Issues a key. Printed once, and recoverable never.
    New {
        /// What this key is for, so a list reads as something.
        label: String,
        /// Pool units it may spend in total, roughly a micro-USD each.
        /// Uncapped when absent; an envelope's ceiling still bounds it.
        #[arg(long)]
        budget: Option<u64>,
    },
    /// Every key, what it has spent, and when it last spent it.
    List,
    /// Stops a key authenticating. What it spent stays on the books.
    Revoke { id: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Read before the subscriber exists, so RUST_LOG can come from the file too.
    let environment = match dotenvy::dotenv() {
        Ok(path) => Some(path),
        Err(error) if error.not_found() => None,
        Err(error) => return Err(error.into()),
    };

    match Cli::parse().command {
        Some(Command::Keygen) => return keygen(),
        Some(Command::Deposit { amount }) => {
            tracing_subscriber::fmt::init();
            let commitment = app::deposit(amount).await?;
            println!("{}", hex::encode(commitment.to_bytes()));
            eprintln!("shielded {amount} under this daemon's key");
            return Ok(());
        }
        Some(Command::Key { what }) => return key(what).await,
        None => {}
    }

    tracing_subscriber::fmt::init();
    if let Some(path) = environment {
        tracing::debug!(?path, "read environment file");
    }

    let daemon = Daemon::start(Config::from_env()?).await?;
    tracing::info!(leaves = daemon.index().leaves().await, "daemon started");
    daemon.run().await?;
    tracing::info!("daemon stopped");
    Ok(())
}

/// Prints a fresh owner. Most random 32-byte strings are not canonical field
/// elements, so these cannot be made up by hand.
fn keygen() -> Result<(), Box<dyn Error>> {
    let owner = Owner::random()?;
    println!("VEIL_SPEND_KEY={}", hex::encode(owner.spend.to_bytes()));
    println!("VEIL_VIEW_KEY={}", hex::encode(owner.view.to_bytes()));
    Ok(())
}

async fn key(what: Keys) -> Result<(), Box<dyn Error>> {
    let agents = app::agents().await?;
    match what {
        // The key alone on stdout, so it can be piped; everything else on
        // stderr, so piping it does not carry the commentary along.
        Keys::New { label, budget } => {
            let issued = agents.issue(&label, budget, app::now()).await?;
            println!("{}", issued.key);
            eprintln!(
                "issued {} for {}; it is not stored and cannot be shown again",
                issued.agent.id, issued.agent.label
            );
        }
        Keys::List => {
            println!(
                "{:<16}  {:<20}  {:>7}  {:>10}  {:>10}  {:>12}",
                "ID", "LABEL", "CALLS", "SPENT", "BUDGET", "LAST BILLED"
            );
            for agent in agents.listed().await? {
                println!(
                    "{:<16}  {:<20}  {:>7}  {:>10}  {:>10}  {:>12}",
                    agent.id,
                    if agent.revoked {
                        format!("{} (revoked)", agent.label)
                    } else {
                        agent.label
                    },
                    agent.calls,
                    agent.spent,
                    agent.budget.map_or_else(|| "-".to_owned(), |b| b.to_string()),
                    agent.used.map_or_else(|| "-".to_owned(), |u| u.to_string()),
                );
            }
        }
        Keys::Revoke { id } => {
            if agents.revoke(&id).await? {
                eprintln!("revoked {id}");
            } else {
                return Err(format!("no live key is held for {id}").into());
            }
        }
    }
    Ok(())
}
