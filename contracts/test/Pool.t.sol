// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {PoseidonT3} from "poseidon-solidity/PoseidonT3.sol";
import {PoseidonT5} from "poseidon-solidity/PoseidonT5.sol";
import {SNARK_SCALAR_FIELD} from "@zk-kit/lazy-imt/Constants.sol";
import {Pool, IVerifier} from "../src/Pool.sol";

contract Token is ERC20 {
    constructor() ERC20("Test", "TST") {
        _mint(msg.sender, 1e18);
    }
}

// Asserts the public inputs at call time so the binding can be checked without
// a real proof; the real verifier is exercised in Verifier.t.sol.
contract StubVerifier is IVerifier {
    uint256[7] public expected;
    bool public checking;
    bool public reject;

    function expect(uint256[7] calldata input) external {
        expected = input;
        checking = true;
    }

    function setReject(bool value) external {
        reject = value;
    }

    function verifyProof(
        uint256[8] calldata,
        uint256[2] calldata,
        uint256[2] calldata,
        uint256[7] calldata input
    ) external view {
        require(!reject, "ProofInvalid");
        if (!checking) return;
        for (uint256 i = 0; i < 7; i++) {
            require(input[i] == expected[i], "unexpected public input");
        }
    }
}

contract PoolTest is Test {
    uint256 constant DEPOSIT = 1_000;

    Token token;
    StubVerifier stub;
    Pool pool;

    address alice = address(0xA11CE);
    address bob = address(0xB0B);

    uint256[8] proof;
    uint256[2] pc;
    uint256[2] pok;

    function setUp() public {
        token = new Token();
        stub = new StubVerifier();
        pool = new Pool(IERC20(address(token)), IVerifier(address(stub)));
        assertTrue(token.transfer(alice, 1e9));
        vm.prank(alice);
        token.approve(address(pool), type(uint256).max);
    }

    function shield(uint64 amount) internal returns (uint256 commitment) {
        uint256 npk = uint256(keccak256(abi.encode("npk", amount))) % SNARK_SCALAR_FIELD;
        commitment = PoseidonT5.hash([uint256(5), npk, pool.assetId(), amount]);
        vm.prank(alice);
        pool.shield(npk, amount, hex"1234");
    }

    function ext(address recipient, int256 amount) internal pure returns (Pool.ExtData memory) {
        return Pool.ExtData({
            recipient: recipient,
            extAmount: amount,
            notes: [bytes(hex"aabb"), bytes(hex"ccdd")]
        });
    }

    function transact(uint256 root, uint256[2] memory nullifiers, Pool.ExtData memory data)
        internal
    {
        uint256[2] memory outs = [uint256(111), uint256(222)];
        pool.transact(proof, pc, pok, root, nullifiers, outs, data);
    }

    // ---- asset identity -------------------------------------------------

    function test_AssetIdBindsTheToken() public view {
        assertEq(pool.assetId(), PoseidonT3.hash([uint256(7), uint256(uint160(address(token)))]));
    }

    // ---- shield ---------------------------------------------------------

    function test_ShieldMovesValueAndRecordsRoot() public {
        uint256 commitment = shield(uint64(DEPOSIT));
        assertEq(token.balanceOf(address(pool)), DEPOSIT);
        assertEq(pool.leafCount(), 1);
        assertTrue(pool.knownRoot(pool.currentRoot()));
        assertTrue(commitment != 0);
    }

    function test_ShieldRejectsBadInput() public {
        vm.startPrank(alice);
        vm.expectRevert(Pool.ZeroAmount.selector);
        pool.shield(1, 0, "");

        vm.expectRevert(Pool.NotInField.selector);
        pool.shield(0, 1, "");

        vm.expectRevert(Pool.NotInField.selector);
        pool.shield(SNARK_SCALAR_FIELD, 1, "");

        vm.expectRevert(Pool.NoteTooLarge.selector);
        pool.shield(1, 1, new bytes(129));
        vm.stopPrank();
    }

    // ---- round trip -----------------------------------------------------

    function test_WithdrawRoundTrips() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();

        transact(root, [uint256(7), uint256(8)], ext(bob, -300));

        assertEq(token.balanceOf(bob), 300);
        assertEq(token.balanceOf(address(pool)), DEPOSIT - 300);
        assertTrue(pool.nullifierSpent(7));
        assertTrue(pool.nullifierSpent(8));
        assertEq(pool.leafCount(), 3);
        assertTrue(pool.knownRoot(pool.currentRoot()));
    }

    function test_ShieldedTransferMovesNoTokens() public {
        shield(uint64(DEPOSIT));
        transact(pool.currentRoot(), [uint256(7), uint256(8)], ext(bob, 0));

        assertEq(token.balanceOf(bob), 0);
        assertEq(token.balanceOf(address(pool)), DEPOSIT);
        assertEq(pool.leafCount(), 3);
    }

    // ---- revert paths ---------------------------------------------------

    function test_RejectsReplayedNullifier() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();
        transact(root, [uint256(7), uint256(8)], ext(bob, -100));

        root = pool.currentRoot();
        vm.expectRevert(Pool.NullifierAlreadySpent.selector);
        transact(root, [uint256(7), uint256(9)], ext(bob, -100));
    }

    function test_RejectsDuplicateNullifiersInOneCall() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();
        vm.expectRevert(Pool.DuplicateNullifier.selector);
        transact(root, [uint256(7), uint256(7)], ext(bob, -100));
    }

    function test_RejectsZeroAndUnknownRoot() public {
        shield(uint64(DEPOSIT));
        vm.expectRevert(Pool.NotInField.selector);
        transact(0, [uint256(7), uint256(8)], ext(bob, -100));

        vm.expectRevert(Pool.UnknownRoot.selector);
        transact(12345, [uint256(7), uint256(8)], ext(bob, -100));
    }

    // A root stays valid after later inserts; eviction is what AW-M-02 punished.
    function test_OldRootStaysKnown() public {
        shield(uint64(DEPOSIT));
        uint256 old = pool.currentRoot();
        for (uint64 i = 0; i < 5; i++) {
            shield(10 + i);
        }
        assertTrue(old != pool.currentRoot());
        transact(old, [uint256(7), uint256(8)], ext(bob, -100));
    }

    function test_RejectsZeroNullifier() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();
        vm.expectRevert(Pool.ZeroNullifier.selector);
        transact(root, [uint256(0), uint256(8)], ext(bob, -100));
    }

    function test_RejectsDepositThroughTransact() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();
        vm.expectRevert(Pool.DepositMustUseShield.selector);
        transact(root, [uint256(7), uint256(8)], ext(bob, 100));
    }

    function test_RejectsAmountWiderThanTheCircuit() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();
        vm.expectRevert(Pool.AmountOutOfRange.selector);
        transact(root, [uint256(7), uint256(8)], ext(bob, int256(type(int64).min) - 1));
    }

    function test_RejectsSelfAndBurnRecipient() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();
        vm.expectRevert(Pool.InvalidRecipient.selector);
        transact(root, [uint256(7), uint256(8)], ext(address(pool), -100));

        vm.expectRevert(Pool.InvalidRecipient.selector);
        transact(root, [uint256(7), uint256(8)], ext(address(0), -100));
    }

    function test_RejectsFailedProof() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();
        stub.setReject(true);
        vm.expectRevert(bytes("ProofInvalid"));
        transact(root, [uint256(7), uint256(8)], ext(bob, -100));
    }

    // ---- proof binding --------------------------------------------------

    function test_PublicAmountIsTheFieldInverse() public view {
        assertEq(pool.publicAmount(0), 0);
        assertEq(pool.publicAmount(-1000), SNARK_SCALAR_FIELD - 1000);
    }

    function test_ProvenInputsAreTheDeclaredOnes() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();
        Pool.ExtData memory data = ext(bob, -300);

        stub.expect(
            [root, 7, 8, 111, 222, SNARK_SCALAR_FIELD - 300, pool.extDataHash(data)]
        );
        transact(root, [uint256(7), uint256(8)], data);
    }

    // The hash the pool proves must be the one it would compute for the data it
    // was handed, not a stale or attacker-chosen value.
    function test_RejectsTamperedExtData() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();

        stub.expect(
            [
                root,
                7,
                8,
                111,
                222,
                SNARK_SCALAR_FIELD - 300,
                pool.extDataHash(ext(bob, -300))
            ]
        );
        vm.expectRevert(bytes("unexpected public input"));
        transact(root, [uint256(7), uint256(8)], ext(alice, -300));
    }

    function test_ExtDataHashBindsEveryField() public view {
        Pool.ExtData memory base = ext(bob, -300);
        uint256 h = pool.extDataHash(base);

        assertTrue(h != pool.extDataHash(ext(alice, -300)), "recipient");
        assertTrue(h != pool.extDataHash(ext(bob, -301)), "amount");

        Pool.ExtData memory other = ext(bob, -300);
        other.notes[1] = hex"ccde";
        assertTrue(h != pool.extDataHash(other), "ciphertext");

        // Length prefixes stop bytes moving from one note into the other.
        Pool.ExtData memory shifted = ext(bob, -300);
        shifted.notes[0] = hex"aabbcc";
        shifted.notes[1] = hex"dd";
        assertTrue(h != pool.extDataHash(shifted), "note boundary");

        assertTrue(h < SNARK_SCALAR_FIELD, "not reduced");
    }

    function test_TransactRejectsOversizeNote() public {
        shield(uint64(DEPOSIT));
        uint256 root = pool.currentRoot();
        Pool.ExtData memory big = ext(bob, -100);
        big.notes[0] = new bytes(129);
        vm.expectRevert(Pool.NoteTooLarge.selector);
        transact(root, [uint256(7), uint256(8)], big);
    }

    function test_ExtDataHashBindsChainAndPool() public {
        Pool.ExtData memory data = ext(bob, -300);
        uint256 h = pool.extDataHash(data);

        vm.chainId(block.chainid + 1);
        assertTrue(h != pool.extDataHash(data), "chain id");

        vm.chainId(block.chainid - 1);
        Pool other = new Pool(IERC20(address(token)), IVerifier(address(stub)));
        assertTrue(h != other.extDataHash(data), "pool address");
    }

    // ---- gas ------------------------------------------------------------

    function test_Gas() public {
        uint256 g = gasleft();
        shield(uint64(DEPOSIT));
        emit log_named_uint("shield", g - gasleft());

        uint256 root = pool.currentRoot();
        g = gasleft();
        transact(root, [uint256(7), uint256(8)], ext(bob, -300));
        emit log_named_uint("transact", g - gasleft());
    }
}
