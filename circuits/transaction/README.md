# transaction circuit (2-in / 2-out)

The join-split every private movement goes through. The cryptography is
explained in [`../README.md`](../README.md); this page is the reference.

## Public inputs

Seven field elements, in the order the verifier and `Pool.sol` pass them:

```
root, nullifiers[0], nullifiers[1],
out_commitments[0], out_commitments[1],
public_amount, ext_data_hash
```

## Proof

The gnark worker returns a Groth16 proof with one BSB22 commitment:

```
388 bytes = Ar(64) + Bs(128) + Krs(64) + count(4) + commitment(64) + pok(64)
```

`prover/veil402-gnark/cmd/solidity-calldata` reshapes it into the verifier's
`(uint256[8] proof, uint256[2] commitments, uint256[2] commitmentPok, uint256[7] input)`.

## Test

```bash
nargo test
```

Artifacts are pinned in `prover/veil402-gnark/artifacts/transaction-v4`.
