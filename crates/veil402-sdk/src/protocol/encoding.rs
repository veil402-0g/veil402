use ark_ff::{BigInteger, PrimeField};

pub(crate) fn field_bytes<F: PrimeField>(value: F) -> [u8; 32] {
    let bytes = value.into_bigint().to_bytes_be();
    let mut encoded = [0_u8; 32];
    encoded[32 - bytes.len()..].copy_from_slice(&bytes);
    encoded
}

pub(crate) fn parse_field<F: PrimeField>(encoded: &[u8; 32]) -> Option<F> {
    let value = F::from_be_bytes_mod_order(encoded);
    (field_bytes(value) == *encoded).then_some(value)
}
