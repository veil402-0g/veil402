// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {PoseidonT3} from "poseidon-solidity/PoseidonT3.sol";
import {PoseidonT5} from "poseidon-solidity/PoseidonT5.sol";

// Noir, light-poseidon and poseidon-solidity must agree. Three independent
// implementations agreeing is the assurance this dependency lacks on its own.
contract PoseidonTest is Test {
    uint256[] a;
    uint256[] b;
    uint256[] c;
    uint256[] d;
    uint256[] t3;
    uint256[] t5;
    uint256[] zeros;

    function setUp() public {
        string memory json = vm.readFile("test/fixtures/poseidon.json");
        a = vm.parseJsonUintArray(json, ".a");
        b = vm.parseJsonUintArray(json, ".b");
        c = vm.parseJsonUintArray(json, ".c");
        d = vm.parseJsonUintArray(json, ".d");
        t3 = vm.parseJsonUintArray(json, ".t3");
        t5 = vm.parseJsonUintArray(json, ".t5");
        zeros = vm.parseJsonUintArray(json, ".zeros");
    }

    function test_MatchesCircomlibVectors() public pure {
        assertEq(
            PoseidonT3.hash([uint256(1), 2]),
            0x115cc0f5e7d690413df64c6b9662e9cf2a3617f2743245519e19607a4417189a
        );
        assertEq(
            PoseidonT5.hash([uint256(1), 2, 3, 4]),
            0x299c867db6c1fdd79dcefa40e4510b9837e60ebb1ce0663dbaa525df65250465
        );
    }

    function test_AgreesWithRustAndNoirOnT3() public view {
        for (uint256 i = 0; i < t3.length; i++) {
            assertEq(PoseidonT3.hash([a[i], b[i]]), t3[i], "t3 mismatch");
        }
    }

    function test_AgreesWithRustAndNoirOnT5() public view {
        for (uint256 i = 0; i < t5.length; i++) {
            assertEq(PoseidonT5.hash([a[i], b[i], c[i], d[i]]), t5[i], "t5 mismatch");
        }
    }

    // The circuit's empty leaf is 0, so the tree must use this chain and not
    // a library default seeded from some other constant.
    function test_ZeroChain() public view {
        assertEq(zeros[0], 0);
        uint256 zero = 0;
        for (uint256 i = 1; i < zeros.length; i++) {
            zero = PoseidonT3.hash([zero, zero]);
            assertEq(zero, zeros[i], "zero chain mismatch");
        }
    }

    function test_Gas() public {
        uint256 before = gasleft();
        PoseidonT3.hash([a[0], b[0]]);
        emit log_named_uint("PoseidonT3 gas", before - gasleft());
        before = gasleft();
        PoseidonT5.hash([a[0], b[0], c[0], d[0]]);
        emit log_named_uint("PoseidonT5 gas", before - gasleft());
    }
}
