// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {PoseidonT3} from "poseidon-solidity/PoseidonT3.sol";
import {InternalLazyIMT, LazyIMTData} from "@zk-kit/lazy-imt/InternalLazyIMT.sol";
import {SNARK_SCALAR_FIELD} from "@zk-kit/lazy-imt/Constants.sol";

// Library calls are internal, so a revert needs an external boundary to be
// caught.
contract TreeHarness {
    using InternalLazyIMT for LazyIMTData;

    LazyIMTData tree;

    constructor(uint8 depth) {
        tree._init(depth);
    }

    function insert(uint256 leaf) external {
        tree._insert(leaf);
    }

    function leaves() external view returns (uint40) {
        return tree.numberOfLeaves;
    }
}

// The pool tree is zk-kit's LazyIMT at depth 20 over zero = 0. Its zero chain
// and its left/right ordering both have to match what the circuit recomputes,
// or every membership proof fails on chain.
contract MerkleTreeTest is Test {
    using InternalLazyIMT for LazyIMTData;

    uint8 constant DEPTH = 20;

    LazyIMTData tree;
    uint256[] zeros;

    function setUp() public {
        zeros = vm.parseJsonUintArray(vm.readFile("test/fixtures/poseidon.json"), ".zeros");
        tree._init(DEPTH);
    }

    // The gate. zk-kit ships these as constants; our chain comes from
    // light-poseidon via the corpus, and Noir agrees with that corpus.
    function test_DefaultZeroesMatchCircuit() public view {
        for (uint8 i = 0; i <= DEPTH; i++) {
            assertEq(InternalLazyIMT._defaultZero(i), zeros[i], "zero chain diverges");
        }
    }

    function test_EmptyRootIsTopOfZeroChain() public view {
        assertEq(tree._root(DEPTH), zeros[DEPTH]);
    }

    // Leaf 0 is a left child at every level, leaf 1 is a right child at level 0.
    // Together they pin the ordering the circuit assumes.
    function test_InsertsMatchCircuitPath() public {
        uint256 leaf = uint256(keccak256("commitment")) % SNARK_SCALAR_FIELD;
        tree._insert(leaf);
        assertEq(tree._root(DEPTH), pathRoot(leaf, 0, new uint256[](0)));

        uint256 second = uint256(keccak256("second")) % SNARK_SCALAR_FIELD;
        tree._insert(second);
        uint256[] memory siblings = new uint256[](1);
        siblings[0] = leaf;
        assertEq(tree._root(DEPTH), pathRoot(second, 1, siblings));
        assertEq(tree.numberOfLeaves, 2);
    }

    // The root must be read at a pinned depth. The no-argument _root() sizes
    // the tree to its current leaf count and returns a different value.
    function test_DynamicDepthRootIsNotThePoolRoot() public {
        tree._insert(1);
        assertTrue(tree._root() != tree._root(DEPTH));
    }

    // A full tree must refuse the next leaf rather than wrap onto an old one.
    // zk-kit stops one short of 2**depth, so capacity is 2**depth - 1.
    function test_FullTreeRefusesMoreLeaves() public {
        TreeHarness small = new TreeHarness(3);
        for (uint256 i = 1; i < 8; i++) {
            small.insert(i);
        }
        assertEq(small.leaves(), 7);
        vm.expectRevert(bytes("LazyIMT: tree is full"));
        small.insert(8);
    }

    function test_RejectsLeafOutsideTheField() public {
        TreeHarness harness = new TreeHarness(DEPTH);
        vm.expectRevert(bytes("LazyIMT: leaf must be < SNARK_SCALAR_FIELD"));
        harness.insert(SNARK_SCALAR_FIELD);
    }

    // Two commitments and one root read is one pool transaction.
    function test_PoolPatternGas() public {
        tree._insert(1);
        tree._insert(2);
        tree._root(DEPTH);

        uint256 before = gasleft();
        tree._insert(3);
        tree._insert(4);
        tree._root(DEPTH);
        emit log_named_uint("two inserts + root", before - gasleft());
    }

    // The circuit's merkle_root: bit set means the node is the right child.
    // Levels past `siblings` are empty subtrees, so they take the zero chain.
    function pathRoot(uint256 leaf, uint256 index, uint256[] memory siblings)
        internal
        view
        returns (uint256)
    {
        uint256 node = leaf;
        for (uint256 level = 0; level < DEPTH; level++) {
            uint256 sibling = level < siblings.length ? siblings[level] : zeros[level];
            node = index & (1 << level) == 0
                ? PoseidonT3.hash([node, sibling])
                : PoseidonT3.hash([sibling, node]);
        }
        return node;
    }
}
