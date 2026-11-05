use poseidon::poseidon::bn254;

global N: u32 = 17;
global DEPTH: u32 = 21;

// Third leg of the Noir / light-poseidon / poseidon-solidity agreement check.
// Inputs come from contracts/test/fixtures/poseidon.json.
fn main(
    a: [Field; N],
    b: [Field; N],
    c: [Field; N],
    d: [Field; N],
    t3: [Field; N],
    t5: [Field; N],
    zeros: [Field; DEPTH],
) {
    for i in 0..N {
        assert(bn254::hash_2([a[i], b[i]]) == t3[i], "t3 mismatch");
        assert(bn254::hash_4([a[i], b[i], c[i], d[i]]) == t5[i], "t5 mismatch");
    }

    assert(zeros[0] == 0, "empty leaf must be zero");
    let mut zero = 0;
    for i in 1..DEPTH {
        zero = bn254::hash_2([zero, zero]);
        assert(zero == zeros[i], "zero chain mismatch");
    }
}
