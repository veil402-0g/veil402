// Command solidity-calldata converts a raw gnark Groth16 proof into the argument
// layout the exported Solidity verifier expects.
//
// The 388-byte raw proof is Ar(64) ‖ Bs(128) ‖ Krs(64) ‖ count(4) ‖ commitment(64) ‖ pok(64).
// Points are parsed with gnark-crypto so the field encoding is canonical rather than assumed.
package main

import (
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"math/big"
	"os"
	"strings"

	"github.com/consensys/gnark-crypto/ecc/bn254"
)

type fixture struct {
	Artifact string   `json:"artifact"`
	Proof    string   `json:"proof"`
	Input    []string `json:"input"`
}

type calldata struct {
	Proof         [8]string `json:"proof"`
	Commitments   [2]string `json:"commitments"`
	CommitmentPok [2]string `json:"commitmentPok"`
	Input         []string  `json:"input"`
}

const rawProofLen = 388

func main() {
	in := flag.String("fixture", "", "path to a proof fixture produced by the SDK test")
	out := flag.String("out", "", "path to write Solidity calldata JSON")
	flag.Parse()
	if *in == "" || *out == "" {
		fmt.Fprintln(os.Stderr, "usage: solidity-calldata -fixture <path> -out <path>")
		os.Exit(2)
	}

	blob, err := os.ReadFile(*in)
	if err != nil {
		fail(err)
	}
	var f fixture
	if err := json.Unmarshal(blob, &f); err != nil {
		fail(err)
	}
	raw, err := hex.DecodeString(strings.TrimPrefix(f.Proof, "0x"))
	if err != nil {
		fail(err)
	}
	if len(raw) != rawProofLen {
		fail(fmt.Errorf("expected %d proof bytes, got %d", rawProofLen, len(raw)))
	}

	var ar, krs, commitment, pok bn254.G1Affine
	var bs bn254.G2Affine
	if _, err := ar.SetBytes(raw[0:64]); err != nil {
		fail(fmt.Errorf("Ar: %w", err))
	}
	if _, err := bs.SetBytes(raw[64:192]); err != nil {
		fail(fmt.Errorf("Bs: %w", err))
	}
	if _, err := krs.SetBytes(raw[192:256]); err != nil {
		fail(fmt.Errorf("Krs: %w", err))
	}
	if n := binary.BigEndian.Uint32(raw[256:260]); n != 1 {
		fail(fmt.Errorf("expected exactly one commitment, got %d", n))
	}
	if _, err := commitment.SetBytes(raw[260:324]); err != nil {
		fail(fmt.Errorf("commitment: %w", err))
	}
	if _, err := pok.SetBytes(raw[324:388]); err != nil {
		fail(fmt.Errorf("commitmentPok: %w", err))
	}

	// EIP-197 orders the quadratic extension imaginary part first.
	result := calldata{
		Proof: [8]string{
			hexInt(ar.X.BigInt(new(big.Int))),
			hexInt(ar.Y.BigInt(new(big.Int))),
			hexInt(bs.X.A1.BigInt(new(big.Int))),
			hexInt(bs.X.A0.BigInt(new(big.Int))),
			hexInt(bs.Y.A1.BigInt(new(big.Int))),
			hexInt(bs.Y.A0.BigInt(new(big.Int))),
			hexInt(krs.X.BigInt(new(big.Int))),
			hexInt(krs.Y.BigInt(new(big.Int))),
		},
		Commitments: [2]string{
			hexInt(commitment.X.BigInt(new(big.Int))),
			hexInt(commitment.Y.BigInt(new(big.Int))),
		},
		CommitmentPok: [2]string{
			hexInt(pok.X.BigInt(new(big.Int))),
			hexInt(pok.Y.BigInt(new(big.Int))),
		},
		Input: f.Input,
	}

	encoded, err := json.MarshalIndent(result, "", "  ")
	if err != nil {
		fail(err)
	}
	if err := os.WriteFile(*out, append(encoded, '\n'), 0o644); err != nil {
		fail(err)
	}
	fmt.Printf("public inputs  %d\nwritten        %s\n", len(f.Input), *out)
}

func hexInt(v *big.Int) string { return "0x" + v.Text(16) }

func fail(err error) {
	fmt.Fprintln(os.Stderr, "error:", err)
	os.Exit(1)
}
