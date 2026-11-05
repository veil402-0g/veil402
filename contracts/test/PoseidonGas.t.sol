// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {PoseidonT3} from "poseidon-solidity/PoseidonT3.sol";

contract PoseidonGasTest is Test {
    // Unrolling leaves PoseidonT3 ~1KB under EIP-170. A solc bump must fail
    // here, not at deploy time.
    function test_FitsCodeSizeLimit() public {
        assertLt(address(PoseidonT3).code.length, 24576, "PoseidonT3 over EIP-170");
    }

    function test_WarmMarginalCost() public {
        PoseidonT3.hash([uint256(1), 2]); // warm the library account

        uint256 before = gasleft();
        PoseidonT3.hash([uint256(3), 4]);
        emit log_named_uint("T3 warm, single", before - gasleft());

        // 20 chained hashes is exactly one depth-20 insert.
        uint256 node = 1;
        before = gasleft();
        for (uint256 i = 0; i < 20; i++) {
            node = PoseidonT3.hash([node, i]);
        }
        emit log_named_uint("20 chained (one insert)", before - gasleft());
    }
}
