// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {EnclaveRegistry} from "../src/EnclaveRegistry.sol";
import {Pool, IEnclaveRegistry, IVerifier} from "../src/Pool.sol";

// The SDK builds the proof and the pool recomputes what it was bound to. If the
// two ever disagree, every transaction reverts at the verifier with nothing to
// say why.
contract BindingTest is Test {
    uint256 constant ENCLAVE_KEY = 0xE11C;

    string json;
    Pool pool;
    EnclaveRegistry registry;

    function setUp() public {
        json = vm.readFile("test/fixtures/binding.json");
        // extDataHash reads address(this), so the pool has to answer at the
        // address the fixture was generated for.
        registry = new EnclaveRegistry(address(this));
        Pool deployed = new Pool(
            IERC20(vm.parseJsonAddress(json, ".token")),
            IVerifier(vm.parseJsonAddress(json, ".verifier")),
            IEnclaveRegistry(address(registry))
        );
        address target = vm.parseJsonAddress(json, ".pool");
        vm.etch(target, address(deployed).code);
        pool = Pool(target);
        vm.chainId(vm.parseJsonUint(json, ".chainId"));
    }

    function test_AssetIdsMatchRust() public {
        address[] memory tokens = vm.parseJsonAddressArray(json, ".tokens");
        uint256[] memory expected = vm.parseJsonUintArray(json, ".assetIds");
        assertEq(tokens.length, expected.length);

        for (uint256 i = 0; i < tokens.length; i++) {
            Pool instance = new Pool(
                IERC20(tokens[i]),
                IVerifier(address(1)),
                IEnclaveRegistry(address(new EnclaveRegistry(address(this))))
            );
            assertEq(instance.assetId(), expected[i], "asset id diverges");
        }
    }

    function test_ExtDataHashesMatchRust() public view {
        address[] memory recipients = vm.parseJsonAddressArray(json, ".recipients");
        int256[] memory amounts = vm.parseJsonIntArray(json, ".amounts");
        bytes[] memory first = vm.parseJsonBytesArray(json, ".notes0");
        bytes[] memory second = vm.parseJsonBytesArray(json, ".notes1");
        uint256[] memory expected = vm.parseJsonUintArray(json, ".hashes");
        assertEq(recipients.length, expected.length);

        for (uint256 i = 0; i < recipients.length; i++) {
            Pool.ExtData memory ext = Pool.ExtData({
                recipient: recipients[i],
                extAmount: amounts[i],
                notes: [first[i], second[i]]
            });
            assertEq(pool.extDataHash(ext), expected[i], "ext data hash diverges");
        }
    }

    function sign(string memory path) internal view returns (bytes memory) {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(ENCLAVE_KEY, vm.parseJsonBytes32(json, path));
        return abi.encodePacked(r, s, v);
    }

    function nullifier() internal view returns (uint256) {
        return vm.parseJsonUint(json, ".escrow.nullifier");
    }

    function seller() internal view returns (address) {
        return vm.parseJsonAddress(json, ".escrow.seller");
    }

    // A signature made over the SDK's digest has to be the one the pool
    // recovers. A mismatched domain or type string would stay invisible until
    // a live settlement failed.
    function test_LockAcceptsTheSdkDigest() public {
        registry.register(vm.addr(ENCLAVE_KEY), keccak256("PCR0"));
        uint64 deadline = uint64(vm.parseJsonUint(json, ".escrow.deadline"));
        vm.warp(deadline - 10 minutes);

        pool.lock(nullifier(), vm.addr(ENCLAVE_KEY), seller(), deadline, sign(".escrow.lockDigest"));

        (address enclave,,,) = pool.escrows(nullifier());
        assertEq(enclave, vm.addr(ENCLAVE_KEY));
    }

    // Reaching the root check means the signature was accepted; a digest
    // mismatch would stop at WrongEnclave instead.
    function test_SettlementAcceptsTheSdkDigest() public {
        test_LockAcceptsTheSdkDigest();

        uint256[8] memory proof;
        uint256[2] memory pair;

        vm.expectRevert(Pool.UnknownRoot.selector);
        pool.settleToAddress(
            proof,
            pair,
            pair,
            12345,
            [nullifier(), uint256(8)],
            pair,
            Pool.ExtData({
                recipient: seller(),
                extAmount: -int256(vm.parseJsonUint(json, ".escrow.amount")),
                notes: [bytes(""), bytes("")]
            }),
            vm.parseJsonBytes32(json, ".escrow.callsRoot"),
            sign(".escrow.settlementDigest")
        );
    }
}
