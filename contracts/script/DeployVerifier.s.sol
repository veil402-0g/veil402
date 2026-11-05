// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Script, console} from "forge-std/Script.sol";
import {Verifier} from "../src/Verifier.sol";
import {PoseidonT3} from "poseidon-solidity/PoseidonT3.sol";
import {PoseidonT5} from "poseidon-solidity/PoseidonT5.sol";

// Deploys the pieces Pool will link against, and proves one real transaction
// on Hedera so the gas figure comes from the target chain rather than anvil.
contract DeployVerifier is Script {
    function run() external {
        string memory json = vm.readFile("test/fixtures/calldata.json");
        uint256[] memory p = vm.parseJsonUintArray(json, ".proof");
        uint256[] memory c = vm.parseJsonUintArray(json, ".commitments");
        uint256[] memory k = vm.parseJsonUintArray(json, ".commitmentPok");
        uint256[] memory i = vm.parseJsonUintArray(json, ".input");

        uint256[8] memory proof;
        uint256[2] memory commitments;
        uint256[2] memory pok;
        uint256[7] memory input;
        for (uint256 n = 0; n < 8; n++) proof[n] = p[n];
        for (uint256 n = 0; n < 2; n++) commitments[n] = c[n];
        for (uint256 n = 0; n < 2; n++) pok[n] = k[n];
        for (uint256 n = 0; n < 7; n++) input[n] = i[n];

        vm.startBroadcast();

        Verifier verifier = new Verifier();
        console.log("Verifier      ", address(verifier));
        console.log("PoseidonT3    ", address(PoseidonT3));
        console.log("PoseidonT5    ", address(PoseidonT5));

        uint256 before = gasleft();
        verifier.verifyProof(proof, commitments, pok, input);
        console.log("verifyProof gas", before - gasleft());

        console.log("PoseidonT3(1,2)", PoseidonT3.hash([uint256(1), 2]));

        vm.stopBroadcast();
    }
}
