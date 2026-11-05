// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {Pool, IVerifier} from "../src/Pool.sol";

// The SDK builds the proof and the pool recomputes what it was bound to. If the
// two ever disagree, every transaction reverts at the verifier with nothing to
// say why.
contract BindingTest is Test {
    string json;
    Pool pool;

    function setUp() public {
        json = vm.readFile("test/fixtures/binding.json");
        // extDataHash reads address(this), so the pool has to answer at the
        // address the fixture was generated for.
        Pool deployed = new Pool(
            IERC20(vm.parseJsonAddress(json, ".token")),
            IVerifier(vm.parseJsonAddress(json, ".verifier"))
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
            Pool instance = new Pool(IERC20(tokens[i]), IVerifier(address(1)));
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
}
