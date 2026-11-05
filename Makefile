NARGO ?= $(HOME)/.nargo/bin/nargo
export PATH := $(HOME)/.foundry/bin:$(PATH)

.PHONY: deps abi corpus poseidon test integration

# contracts/lib is gitignored, so a fresh clone has no dependencies at all.
# Pinned by commit: zk-kit's zero-chain constants have to stay fixed.
define clone
	@test -d contracts/lib/$(1) || git clone -q $(2) contracts/lib/$(1)
	@git -C contracts/lib/$(1) fetch -q origin $(3) && git -C contracts/lib/$(1) checkout -q $(3)
endef

deps:
	$(call clone,forge-std,https://github.com/foundry-rs/forge-std,bd8c87087369377df8b451de74fb3a4fa9a7f577)
	$(call clone,poseidon-solidity,https://github.com/chancehudson/poseidon-solidity,8205c97ec4a35a9adb854b70b2c9ba92668d1304)
	$(call clone,openzeppelin-contracts,https://github.com/OpenZeppelin/openzeppelin-contracts,c64a1edb67b6e3f4a15cca8909c9482ad33a02b0)
	$(call clone,zk-kit.solidity,https://github.com/privacy-scaling-explorations/zk-kit.solidity,7cde10ed96629ed1fdf990c607334842e0a24f93)

# The SDK generates its bindings from this, so it must track the contract.
abi:
	cd contracts && forge inspect Pool abi --json > ../crates/veil402-sdk/abi/pool.json

corpus:
	cargo run --quiet --manifest-path tools/poseidon-corpus/Cargo.toml -- $(CURDIR)

# Noir, light-poseidon and poseidon-solidity must agree.
poseidon: corpus
	cd circuits/poseidon-check && $(NARGO) execute
	cd contracts && forge test --match-path "test/Poseidon*.t.sol"

# Spending the first note relies on MerklePath::only_leaf, so each case needs a
# tree nobody has touched.
RPC ?= http://127.0.0.1:8545
KEY ?= 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80

integration: abi
	@for case in shields_then_withdraws meters_a_session_and_settles spends_the_second_note an_envelope_rolls_into_the_next; do \
	  out=$$(cd contracts && forge script script/DeployPool.s.sol:DeployPool \
	    --rpc-url $(RPC) --private-key $(KEY) --broadcast 2>&1); \
	  RPC_URL=$(RPC) PRIVATE_KEY=$(KEY) \
	  POOL=$$(echo "$$out" | awk '/pool /{print $$2}') \
	  TOKEN=$$(echo "$$out" | awk '/token /{print $$2}') \
	  VERIFIER=$$(echo "$$out" | awk '/verifier /{print $$2}') \
	  REGISTRY=$$(echo "$$out" | awk '/registry /{print $$2}') \
	  cargo test --workspace --test integration --test live \
	    $$case -- --ignored --nocapture; \
	done

test: abi poseidon
	cargo test --manifest-path crates/veil402-sdk/Cargo.toml
	cd contracts && forge test
