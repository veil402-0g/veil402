// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {Verifier} from "../src/Verifier.sol";

/// Phase 0 gate: a Groth16 proof produced by the existing Noir/gnark stack —
/// unchanged from the Solana bundle — must verify under the EVM's ecPairing
/// precompile. If this passes, the whole money layer is already written.
contract VerifierTest is Test {
    Verifier internal verifier;

    uint256[8] internal proof;
    uint256[2] internal commitments;
    uint256[2] internal commitmentPok;
    uint256[7] internal input;

    function setUp() public {
        verifier = new Verifier();

        string memory json = vm.readFile("test/fixtures/calldata.json");
        uint256[] memory p = vm.parseJsonUintArray(json, ".proof");
        uint256[] memory c = vm.parseJsonUintArray(json, ".commitments");
        uint256[] memory k = vm.parseJsonUintArray(json, ".commitmentPok");
        uint256[] memory i = vm.parseJsonUintArray(json, ".input");

        assertEq(p.length, 8, "proof length");
        assertEq(c.length, 2, "commitments length");
        assertEq(k.length, 2, "commitmentPok length");
        assertEq(i.length, 7, "public input length");

        for (uint256 n = 0; n < 8; n++) proof[n] = p[n];
        for (uint256 n = 0; n < 2; n++) commitments[n] = c[n];
        for (uint256 n = 0; n < 2; n++) commitmentPok[n] = k[n];
        for (uint256 n = 0; n < 7; n++) input[n] = i[n];
    }

    function test_VerifiesRealProof() public view {
        verifier.verifyProof(proof, commitments, commitmentPok, input);
    }

    function test_RejectsTamperedPublicInput() public {
        uint256[7] memory bad = input;
        bad[5] = bad[5] ^ 1; // flip a bit in public_amount
        vm.expectRevert();
        verifier.verifyProof(proof, commitments, commitmentPok, bad);
    }

    function test_GasCost() public view {
        uint256 before = gasleft();
        verifier.verifyProof(proof, commitments, commitmentPok, input);
        emit log_named_uint("groth16 verify gas", before - gasleft());
    }
}
