//! The two halves of the vsock protocol, against each other.
//!
//! The point is not that either half works alone — it is that a request
//! written by one is the request the other reads. Once the enclave is
//! measured the two binaries ship separately, so a disagreement here is a
//! deployment that cannot be corrected from one side.

use std::{pin::Pin, sync::Arc};

use alloy::primitives::{Address, B256};
use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use veil402_enclave::protocol::LinkError;
use veil402_gateway::adapters::link::{Connect, Duplex, Linked, serve};
use veil402_gateway::core::{
    Accepted, Billed, Kept, MeterError, Meters, Metering, Opened, Replayed,
};
use veil402_sdk::{
    Field,
    evm::Session,
    session::Sealed,
    wire::{AgreeBody, MeteredBody, Offer, SettlementBody, Witness},
};

const SIGNER: [u8; 20] = [7; 20];

/// A meter that answers predictably, so the test is about the crossing.
struct Fake;

#[async_trait]
impl Meters for Fake {
    fn signer(&self) -> Address {
        Address::from(SIGNER)
    }

    async fn catalogue(&self) -> Result<Vec<Offer>, MeterError> {
        Ok(vec![Offer { model: "m".into(), prompt: 1, completion: 2, max_output: 3 }])
    }

    async fn agree(&self, session: &Session) -> Result<AgreeBody, MeterError> {
        Ok(AgreeBody {
            signer: Address::from(SIGNER),
            handover_key: format!("{:?}", session.nullifier.to_bytes()),
            announcement: "a".into(),
            lock: "l".into(),
        })
    }

    async fn accept(&self, _sealed: &Sealed, session: Session) -> Result<Accepted, MeterError> {
        Ok(Accepted { root: session.nullifier })
    }

    async fn release(&self, _nullifier: Field) {}

    async fn holds(&self, _nullifier: Field) -> bool {
        true
    }

    async fn record(
        &self,
        _nullifier: Field,
        attempt: &str,
        _sealed: &Sealed,
    ) -> Result<Opened, MeterError> {
        let bill = Metering {
            metered: MeteredBody {
                cost: 9,
                total: 9,
                remaining: 1,
                attestation: B256::repeat_byte(3),
                delivered: Some(B256::repeat_byte(4)),
                witness: Some(Witness {
                    reference: attempt.to_owned(),
                    prompt: 10,
                    completion: 20,
                    provider: None,
                }),
                sequence: 0,
            },
            record: vec![1, 2, 3],
        };
        Ok(Opened {
            head: [5; 32],
            frames: Box::pin(stream::iter(vec![
                Ok(Billed::Frame(b"one".to_vec())),
                Ok(Billed::Frame(b"two".to_vec())),
                Ok(Billed::Bill(bill)),
            ])),
        })
    }

    async fn resume(&self, _kept: &Kept) -> Result<Vec<Replayed>, MeterError> {
        Ok(vec![Replayed {
            attempt: "a".into(),
            total: 1,
            remaining: 2,
            attestation: B256::repeat_byte(6),
        }])
    }

    async fn settle(&self, _nullifier: Field, dust: bool) -> Result<SettlementBody, MeterError> {
        Ok(SettlementBody { key: if dust { "dust" } else { "full" }.into(), calldata: "cd".into() })
    }
}

/// Dials a fresh in-memory pair and serves one call on the far side, which is
/// what a vsock listener does without the hardware.
struct Loopback(Arc<dyn Meters>);

#[async_trait]
impl Connect for Loopback {
    async fn connect(&self) -> Result<Pin<Box<dyn Duplex>>, LinkError> {
        let (near, far) = tokio::io::duplex(64 * 1024);
        let meters = self.0.clone();
        tokio::spawn(async move {
            let _ = serve(&meters, Box::pin(far)).await;
        });
        Ok(Box::pin(near))
    }
}

fn nullifier() -> Field {
    Field::from_bytes([1; 32]).expect("one is in the field")
}

async fn linked() -> Linked<Loopback> {
    Linked::open(Loopback(Arc::new(Fake)))
        .await
        .expect("the fake answers its signer")
}

#[tokio::test]
async fn carries_every_call_the_meter_answers() {
    let link = linked().await;

    assert_eq!(link.signer(), Address::from(SIGNER), "signer survives the crossing");

    let offers = link.catalogue().await.expect("catalogue");
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].model, "m");

    let session = Session { nullifier: nullifier(), seller: Address::from([2; 20]), deadline: 99 };
    let agreed = link.agree(&session).await.expect("agree");
    assert_eq!(
        agreed.handover_key,
        format!("{:?}", session.nullifier.to_bytes()),
        "the session reached the far side intact"
    );

    let sealed = Sealed { encapsulated: [8; 32], ciphertext: vec![9; 100] };
    let accepted = link.accept(&sealed, session).await.expect("accept");
    assert_eq!(accepted.root, nullifier(), "a field element round-trips");

    assert!(link.holds(nullifier()).await);
    link.release(nullifier()).await;

    let resumed = link.resume(&Kept { sealed, session, log: vec![vec![1]] })
        .await
        .expect("resume");
    assert_eq!(resumed[0].attestation, B256::repeat_byte(6));

    let settled = link.settle(nullifier(), true).await.expect("settle");
    assert_eq!(settled.key, "dust", "the flag is not dropped on the way");
}

#[tokio::test]
async fn streams_the_frames_then_the_bill() {
    let link = linked().await;
    let sealed = Sealed { encapsulated: [8; 32], ciphertext: vec![0; 10] };
    let opened = link.record(nullifier(), "attempt-1", &sealed).await.expect("record");

    assert_eq!(opened.head, [5; 32], "the encapsulated key travels once");

    let got: Vec<_> = opened.frames.collect().await;
    assert_eq!(got.len(), 3, "two frames and a bill, and the stream then ends");

    match &got[0] {
        Ok(Billed::Frame(bytes)) => assert_eq!(bytes, b"one"),
        other => panic!("expected the first frame, got {other:?}", other = other.is_ok()),
    }
    match &got[2] {
        Ok(Billed::Bill(bill)) => {
            assert_eq!(bill.metered.cost, 9);
            assert_eq!(bill.record, vec![1, 2, 3], "the sealed record is not re-encoded");
            let witness = bill.metered.witness.as_ref().expect("witness");
            assert_eq!(witness.reference, "attempt-1", "the attempt reached the meter");
        }
        other => panic!("expected the bill last, got ok={}", other.is_ok()),
    }
}

#[tokio::test]
async fn a_refusal_crosses_as_itself() {
    struct Refuses;

    #[async_trait]
    impl Meters for Refuses {
        fn signer(&self) -> Address {
            Address::from(SIGNER)
        }
        async fn catalogue(&self) -> Result<Vec<Offer>, MeterError> {
            Err(MeterError::CeilingReached)
        }
        async fn agree(&self, _: &Session) -> Result<AgreeBody, MeterError> {
            unreachable!()
        }
        async fn accept(&self, _: &Sealed, _: Session) -> Result<Accepted, MeterError> {
            Err(MeterError::Unreserved("the escrow names another enclave"))
        }
        async fn release(&self, _: Field) {}
        async fn holds(&self, _: Field) -> bool {
            false
        }
        async fn record(&self, _: Field, _: &str, _: &Sealed) -> Result<Opened, MeterError> {
            Err(MeterError::Repeated)
        }
        async fn resume(&self, _: &Kept) -> Result<Vec<Replayed>, MeterError> {
            unreachable!()
        }
        async fn settle(&self, _: Field, _: bool) -> Result<SettlementBody, MeterError> {
            unreachable!()
        }
    }

    let link = Linked::open(Loopback(Arc::new(Refuses))).await.expect("open");

    // The variants the host maps onto distinct statuses must stay apart: a
    // 409 is not a 402, and flattening them to a string would lose that.
    assert!(matches!(link.catalogue().await, Err(MeterError::CeilingReached)));
    let sealed = Sealed { encapsulated: [0; 32], ciphertext: vec![] };
    assert!(matches!(
        link.record(nullifier(), "a", &sealed).await,
        Err(MeterError::Repeated)
    ));
    // Unreserved carries a &'static str, which cannot cross; it arrives as a
    // refusal that still says why.
    match link.accept(&sealed, Session {
        nullifier: nullifier(),
        seller: Address::ZERO,
        deadline: 0,
    }).await {
        Err(MeterError::Refused(why)) => assert!(why.contains("another enclave")),
        other => panic!("expected a refusal, got ok={}", other.is_ok()),
    }
}
