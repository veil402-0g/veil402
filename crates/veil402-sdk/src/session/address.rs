//! Where a wallet can be privately paid.
//!
//! The owner identity sets the output note's key; the viewing key is what a
//! sender seals the note's opening to, so only the payee discovers it. Both
//! are public, so the string is safe to hand over. Borsh for the body, hex to
//! match every other envelope on the wire.

use std::{
    fmt::{self, Display},
    str::FromStr,
};

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{Field, client::Error};

const SCHEME: &str = "veil402:pay:v1:";

#[derive(BorshDeserialize, BorshSerialize)]
struct Wire {
    owner: [u8; 32],
    view: [u8; 32],
}

/// A payee's public owner identity and the viewing key to seal to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaymentAddress {
    pub owner: Field,
    pub view: [u8; 32],
}

impl PaymentAddress {
    #[must_use]
    pub const fn new(owner: Field, view: [u8; 32]) -> Self {
        Self { owner, view }
    }
}

impl Display for PaymentAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let wire = Wire {
            owner: self.owner.to_bytes(),
            view: self.view,
        };
        let bytes = borsh::to_vec(&wire).map_err(|_| fmt::Error)?;
        write!(formatter, "{SCHEME}{}", hex::encode(bytes))
    }
}

impl FromStr for PaymentAddress {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        let body = text
            .strip_prefix(SCHEME)
            .ok_or(Error::Address("not a veil402 pay address"))?;
        let bytes = hex::decode(body).map_err(|_| Error::Address("address is not hex"))?;
        let wire = Wire::try_from_slice(&bytes).map_err(|_| Error::Address("address is malformed"))?;
        Ok(Self {
            owner: Field::from_bytes(wire.owner).map_err(|_| Error::Address("owner is not a field element"))?,
            view: wire.view,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_string_form() {
        let address = PaymentAddress::new(Field::from_raw_u64(7), [3_u8; 32]);
        let text = address.to_string();
        assert!(text.starts_with(SCHEME));
        assert_eq!(text.parse::<PaymentAddress>().unwrap(), address);
    }

    #[test]
    fn rejects_a_foreign_prefix_and_a_corrupt_body() {
        assert!("veil402:note:v1:abcd".parse::<PaymentAddress>().is_err());
        assert!("veil402:pay:v1:zz".parse::<PaymentAddress>().is_err());
        assert!("veil402:pay:v1:00".parse::<PaymentAddress>().is_err());
    }
}
