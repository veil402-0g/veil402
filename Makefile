NARGO ?= $(HOME)/.nargo/bin/nargo
export PATH := $(HOME)/.foundry/bin:$(PATH)

.PHONY: corpus poseidon test

corpus:
	cargo run --quiet --manifest-path tools/poseidon-corpus/Cargo.toml -- $(CURDIR)

# Noir, light-poseidon and poseidon-solidity must agree.
poseidon: corpus
	cd circuits/poseidon-check && $(NARGO) execute
	cd contracts && forge test --match-path "test/Poseidon*.t.sol"

test: poseidon
	cargo test --manifest-path crates/veil402-sdk/Cargo.toml
	cd contracts && forge test
