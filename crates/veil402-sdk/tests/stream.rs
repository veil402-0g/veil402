//! An answer that arrives in pieces, over a relay that may read none of them.
//!
//! The relay here is the gateway: it forwards frames it cannot open, so the
//! only edits available to it are the ones a transport can make — reorder,
//! drop, repeat, alter, or stop early. Each one has to fail.
//!
//! Whether a frame was the last is sealed with it, so there is no version of
//! these tests where a relay gets to say.

use veil402_sdk::session::{EnclaveKey, Exchange};

type Fallible = Result<(), Box<dyn std::error::Error>>;

struct Answer {
    head: [u8; 32],
    frames: Vec<Vec<u8>>,
}

/// Three frames of an answer, the last of them marked as such.
fn answer(to: [u8; 32]) -> Result<Answer, Box<dyn std::error::Error>> {
    let mut outbound = EnclaveKey::reply_stream(to)?;
    let head = outbound.head();
    let frames = vec![
        outbound.frame(b"the ", false)?,
        outbound.frame(b"whole ", false)?,
        outbound.frame(b"answer", true)?,
    ];
    Ok(Answer { head, frames })
}

#[test]
fn a_sealed_stream_opens_in_order_and_not_out_of_it() -> Fallible {
    let exchange = Exchange::open();
    let Answer { head, frames } = answer(exchange.public())?;

    let mut inbound = exchange.inbound(head)?;
    assert_eq!(inbound.frame(&frames[0])?.payload, b"the ");
    assert_eq!(inbound.frame(&frames[1])?.payload, b"whole ");
    let closing = inbound.frame(&frames[2])?;
    assert_eq!(closing.payload, b"answer");
    assert!(closing.last, "the closing frame did not say so");
    assert!(inbound.done());

    let mut inbound = exchange.inbound(head)?;
    assert!(
        inbound.frame(&frames[1]).is_err(),
        "a reordered frame opened"
    );

    let mut inbound = exchange.inbound(head)?;
    inbound.frame(&frames[0])?;
    assert!(
        inbound.frame(&frames[2]).is_err(),
        "a gap in the sequence opened"
    );

    let mut inbound = exchange.inbound(head)?;
    inbound.frame(&frames[0])?;
    assert!(
        inbound.frame(&frames[0]).is_err(),
        "a repeated frame opened"
    );

    let mut altered = frames[0].clone();
    altered[0] ^= 1;
    let mut inbound = exchange.inbound(head)?;
    assert!(
        inbound.frame(&altered).is_err(),
        "an altered frame opened"
    );

    let mut stranger = Exchange::open().inbound(head)?;
    assert!(
        stranger.frame(&frames[0]).is_err(),
        "another channel's key opened it"
    );
    Ok(())
}

/// The one a sequence number cannot catch: every frame that did arrive is
/// genuine, and the answer is still only half of one.
#[test]
fn a_truncated_stream_is_never_mistaken_for_a_whole_one() -> Fallible {
    let exchange = Exchange::open();
    let Answer { head, frames } = answer(exchange.public())?;

    let mut inbound = exchange.inbound(head)?;
    inbound.frame(&frames[0])?;
    inbound.frame(&frames[1])?;
    assert!(!inbound.done(), "a truncated answer passed as a whole one");

    // There is no forging an ending: a relay has nothing to set. The frame
    // that says so is the one the sender sealed saying it.
    let mut inbound = exchange.inbound(head)?;
    assert!(!inbound.frame(&frames[0])?.last);
    assert!(!inbound.frame(&frames[1])?.last);

    let mut inbound = exchange.inbound(head)?;
    inbound.frame(&frames[0])?;
    inbound.frame(&frames[1])?;
    inbound.frame(&frames[2])?;
    assert!(
        inbound.frame(&frames[2]).is_err(),
        "a frame opened after the end"
    );
    Ok(())
}
