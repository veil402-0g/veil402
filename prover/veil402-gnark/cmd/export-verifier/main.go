// Command export-verifier emits a Solidity Groth16 verifier from the pinned
// Veil402 verifying key. The key is unchanged from the Solana bundle: Groth16
// over BN254 verifies identically under the EVM's ecPairing precompile.
package main

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"

	"github.com/consensys/gnark-crypto/ecc"
	"github.com/consensys/gnark/backend/groth16"
	"github.com/consensys/gnark/logger"
)

func main() {
	logger.Disable()
	in := flag.String("vk", "", "path to transaction.vk")
	out := flag.String("out", "", "path to write the Solidity verifier")
	flag.Parse()
	if *in == "" || *out == "" {
		fmt.Fprintln(os.Stderr, "usage: export-verifier -vk <path> -out <path>")
		os.Exit(2)
	}

	vk := groth16.NewVerifyingKey(ecc.BN254)
	f, err := os.Open(*in)
	if err != nil {
		fail(err)
	}
	defer f.Close()
	if _, err := vk.ReadFrom(f); err != nil {
		fail(err)
	}

	if err := os.MkdirAll(filepath.Dir(*out), 0o755); err != nil {
		fail(err)
	}
	w, err := os.Create(*out)
	if err != nil {
		fail(err)
	}
	defer w.Close()
	if err := vk.ExportSolidity(w); err != nil {
		fail(err)
	}

	fmt.Printf("curve             BN254\n")
	fmt.Printf("public inputs     %d\n", vk.NbPublicWitness())
	fmt.Printf("written           %s\n", *out)
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, "error:", err)
	os.Exit(1)
}
