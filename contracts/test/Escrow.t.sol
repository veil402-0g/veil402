// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SNARK_SCALAR_FIELD} from "@zk-kit/lazy-imt/Constants.sol";
import {EnclaveRegistry} from "../src/EnclaveRegistry.sol";
import {Pool, IEnclaveRegistry, IVerifier} from "../src/Pool.sol";
import {Token, StubVerifier} from "./Pool.t.sol";

// An escrow reserves a nullifier; the pool never holds the value. The note
// stays the buyer's, it just cannot be spent elsewhere while the session runs.
contract EscrowTest is Test {
    bytes32 constant CALLS_ROOT = keccak256("412 calls");
    uint256 constant SPEND = 300;

    Token token;
    StubVerifier stub;
    EnclaveRegistry registry;
    Pool pool;

    uint256 enclaveKey = 0xE11C;
    address enclave;
    address seller = address(0x5E11E2);
    address alice = address(0xA11CE);

    uint256 constant NULLIFIER = 7;
    uint64 deadline;
    // vm.expectRevert arms the next external call, so the root is read here
    // rather than inside a helper's argument list.
    uint256 root;

    uint256[8] proof;
    uint256[2] pc;
    uint256[2] pok;

    function setUp() public {
        enclave = vm.addr(enclaveKey);
        token = new Token();
        stub = new StubVerifier();
        registry = new EnclaveRegistry(address(this));
        pool = new Pool(
            IERC20(address(token)), IVerifier(address(stub)), IEnclaveRegistry(address(registry))
        );
        registry.register(enclave, keccak256("PCR0"));
        assertTrue(token.transfer(alice, 1e9));
        vm.prank(alice);
        token.approve(address(pool), type(uint256).max);

        vm.warp(1_700_000_000);
        deadline = uint64(block.timestamp + 10 minutes);

        // A note must exist for its root to be known.
        vm.prank(alice);
        pool.shield(uint256(keccak256("npk")) % SNARK_SCALAR_FIELD, 1_000, hex"00");
        root = pool.currentRoot();
    }

    // ---- helpers --------------------------------------------------------

    function ext(address recipient, int256 amount) internal pure returns (Pool.ExtData memory) {
        return Pool.ExtData({
            recipient: recipient,
            extAmount: amount,
            notes: [bytes(hex"aabb"), bytes(hex"ccdd")]
        });
    }

    function digest(bytes32 structHash) internal view returns (bytes32) {
        bytes32 domain = keccak256(
            abi.encode(
                keccak256(
                    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
                ),
                keccak256("Veil402Pool"),
                keccak256("1"),
                block.chainid,
                address(pool)
            )
        );
        return keccak256(abi.encodePacked("\x19\x01", domain, structHash));
    }

    function signAddress(uint256 key, uint256 nullifier, uint256 amount)
        internal
        view
        returns (bytes memory)
    {
        bytes32 structHash = keccak256(
            abi.encode(
                keccak256("Settlement(uint256 nullifier,uint256 amount,bytes32 callsRoot)"),
                nullifier,
                amount,
                CALLS_ROOT
            )
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(key, digest(structHash));
        return abi.encodePacked(r, s, v);
    }

    function signNote(uint256 key, uint256 nullifier, uint256 commitment)
        internal
        view
        returns (bytes memory)
    {
        bytes32 structHash = keccak256(
            abi.encode(
                keccak256(
                    "NoteSettlement(uint256 nullifier,uint256 sellerCommitment,bytes32 callsRoot)"
                ),
                nullifier,
                commitment,
                CALLS_ROOT
            )
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(key, digest(structHash));
        return abi.encodePacked(r, s, v);
    }

    function signLock(uint256 key, uint256 nullifier, address to, uint64 until)
        internal
        view
        returns (bytes memory)
    {
        bytes32 structHash = keccak256(
            abi.encode(
                keccak256("Lock(uint256 nullifier,address seller,uint64 deadline)"),
                nullifier,
                to,
                until
            )
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(key, digest(structHash));
        return abi.encodePacked(r, s, v);
    }

    function lock() internal {
        pool.lock(
            NULLIFIER, enclave, seller, deadline, signLock(enclaveKey, NULLIFIER, seller, deadline)
        );
    }

    function nullifiers() internal pure returns (uint256[2] memory) {
        return [NULLIFIER, uint256(8)];
    }

    function outs() internal pure returns (uint256[2] memory) {
        return [uint256(111), uint256(222)];
    }

    function settleToAddress(bytes memory signature) internal {
        pool.settleToAddress(
            proof,
            pc,
            pok,
            root,
            nullifiers(),
            outs(),
            ext(seller, -int256(SPEND)),
            CALLS_ROOT,
            signature
        );
    }

    // ---- lock -----------------------------------------------------------

    function test_LockReservesTheNullifier() public {
        lock();
        (address e, address s, uint64 d, bool settled) = pool.escrows(NULLIFIER);
        assertEq(e, enclave);
        assertEq(s, seller);
        assertEq(d, deadline);
        assertFalse(settled);
    }

    function test_LockRejectsBadInput() public {
        bytes memory signature = signLock(enclaveKey, NULLIFIER, seller, deadline);

        vm.expectRevert(Pool.ZeroNullifier.selector);
        pool.lock(0, enclave, seller, deadline, signature);

        vm.expectRevert(Pool.NotInField.selector);
        pool.lock(SNARK_SCALAR_FIELD, enclave, seller, deadline, signature);

        vm.expectRevert(Pool.InvalidRecipient.selector);
        pool.lock(NULLIFIER, address(0), seller, deadline, signature);

        vm.expectRevert(Pool.InvalidRecipient.selector);
        pool.lock(NULLIFIER, enclave, address(0), deadline, signature);

        // Paying the pool itself would burn the seller's money.
        vm.expectRevert(Pool.InvalidRecipient.selector);
        pool.lock(NULLIFIER, enclave, address(pool), deadline, signature);
    }

    // The enclave has to agree to meter the session, so a reservation is always
    // attributable to a key buyers pin and we can revoke.
    function test_LockRequiresEnclaveConsent() public {
        vm.expectRevert(Pool.WrongEnclave.selector);
        pool.lock(
            NULLIFIER, enclave, seller, deadline, signLock(0xBAD, NULLIFIER, seller, deadline)
        );

        // Every term is covered, so none can be swapped after signing.
        vm.expectRevert(Pool.WrongEnclave.selector);
        pool.lock(
            NULLIFIER, enclave, alice, deadline, signLock(enclaveKey, NULLIFIER, seller, deadline)
        );

        vm.expectRevert(Pool.WrongEnclave.selector);
        pool.lock(
            NULLIFIER, enclave, seller, deadline, signLock(enclaveKey, 9, seller, deadline)
        );

        vm.expectRevert(Pool.WrongEnclave.selector);
        pool.lock(
            NULLIFIER,
            enclave,
            seller,
            deadline,
            signLock(enclaveKey, NULLIFIER, seller, deadline - 1)
        );
    }

    function test_LockRejectsBadDeadline() public {
        uint64 maxLock = pool.MAX_LOCK();
        uint64 now_ = uint64(block.timestamp);
        bytes memory past = signLock(enclaveKey, NULLIFIER, seller, now_);
        bytes memory far = signLock(enclaveKey, NULLIFIER, seller, now_ + maxLock + 1);

        vm.expectRevert(Pool.BadDeadline.selector);
        pool.lock(NULLIFIER, enclave, seller, now_, past);

        // A long lock would strand a front-run note for longer.
        vm.expectRevert(Pool.BadDeadline.selector);
        pool.lock(NULLIFIER, enclave, seller, now_ + maxLock + 1, far);
    }

    function test_LockRejectsDoubleLock() public {
        lock();
        vm.expectRevert(Pool.AlreadyLocked.selector);
        lock();
    }

    function test_LockRequiresALiveEnclave() public {
        address stranger = vm.addr(0xBAD);
        bytes memory signature = signLock(0xBAD, NULLIFIER, seller, deadline);
        vm.expectRevert(Pool.EnclaveNotLive.selector);
        pool.lock(NULLIFIER, stranger, seller, deadline, signature);

        registry.revoke(enclave);
        bytes memory signed = signLock(enclaveKey, NULLIFIER, seller, deadline);
        vm.expectRevert(Pool.EnclaveNotLive.selector);
        pool.lock(NULLIFIER, enclave, seller, deadline, signed);
    }

    // Revoking a key must not strand sessions already running under it, or the
    // seller would lose work it had already delivered.
    function test_RevocationDoesNotStrandALiveEscrow() public {
        lock();
        registry.revoke(enclave);

        settleToAddress(signAddress(enclaveKey, NULLIFIER, SPEND));
        assertEq(token.balanceOf(seller), SPEND);
    }

    // Settling one escrow must not quietly consume another's note: the second
    // escrow's enclave never authorised that spend.
    function test_SettleCannotConsumeASecondEscrow() public {
        lock();
        uint256 other = nullifiers()[1];
        pool.lock(other, enclave, seller, deadline, signLock(enclaveKey, other, seller, deadline));

        vm.expectRevert(Pool.NullifierReserved.selector);
        settleToAddress(signAddress(enclaveKey, NULLIFIER, SPEND));
    }

    // ---- reservation ----------------------------------------------------

    function test_ReservedNullifierCannotBeSpentNormally() public {
        lock();
        vm.expectRevert(Pool.NullifierReserved.selector);
        pool.transact(proof, pc, pok, root, nullifiers(), outs(), ext(alice, -100));
    }

    // The buyer gets the note back only after the grace window, so a late
    // settlement can never race them for it.
    function test_GraceWindowHasNoOverlap() public {
        lock();
        uint64 grace = pool.SETTLE_GRACE();
        bytes memory signature = signAddress(enclaveKey, NULLIFIER, SPEND);

        // At the deadline the seller may still settle, and the buyer may not.
        vm.warp(deadline);
        vm.expectRevert(Pool.NullifierReserved.selector);
        pool.transact(proof, pc, pok, root, nullifiers(), outs(), ext(alice, -100));

        // One second later the seller is out of time...
        vm.warp(uint256(deadline) + 1);
        vm.expectRevert(Pool.EscrowExpired.selector);
        settleToAddress(signature);

        // ...but the buyer still cannot act until the grace window closes.
        vm.warp(uint256(deadline) + grace);
        vm.expectRevert(Pool.NullifierReserved.selector);
        pool.transact(proof, pc, pok, root, nullifiers(), outs(), ext(alice, -100));

        vm.warp(uint256(deadline) + grace + 1);
        pool.transact(proof, pc, pok, root, nullifiers(), outs(), ext(alice, -100));
        assertTrue(pool.nullifierSpent(NULLIFIER));
    }

    // The last instant the seller may act is the deadline itself.
    function test_SellerMaySettleAtTheDeadline() public {
        lock();
        vm.warp(deadline);
        settleToAddress(signAddress(enclaveKey, NULLIFIER, SPEND));
        assertEq(token.balanceOf(seller), SPEND);
    }

    // ---- settleToAddress ------------------------------------------------

    function test_SettlePaysTheSeller() public {
        lock();
        settleToAddress(signAddress(enclaveKey, NULLIFIER, SPEND));

        assertEq(token.balanceOf(seller), SPEND);
        assertTrue(pool.nullifierSpent(NULLIFIER));
        (,,, bool settled) = pool.escrows(NULLIFIER);
        assertTrue(settled);
    }

    function test_SettleRejectsWrongEnclave() public {
        lock();
        vm.expectRevert(Pool.WrongEnclave.selector);
        settleToAddress(signAddress(0xBAD, NULLIFIER, SPEND));
    }

    // The signature covers the amount, so a relayer cannot re-price it.
    function test_SettleRejectsRepricing() public {
        lock();
        vm.expectRevert(Pool.WrongEnclave.selector);
        settleToAddress(signAddress(enclaveKey, NULLIFIER, SPEND + 1));
    }

    function test_SettleRejectsOtherRecipient() public {
        lock();
        vm.expectRevert(Pool.InvalidRecipient.selector);
        pool.settleToAddress(
            proof,
            pc,
            pok,
            root,
            nullifiers(),
            outs(),
            ext(alice, -int256(SPEND)),
            CALLS_ROOT,
            signAddress(enclaveKey, NULLIFIER, SPEND)
        );
    }

    function test_SettleRejectsUnescrowedNullifier() public {
        vm.expectRevert(Pool.NotEscrowed.selector);
        settleToAddress(signAddress(enclaveKey, NULLIFIER, SPEND));
    }

    function test_SettleCannotReplay() public {
        lock();
        settleToAddress(signAddress(enclaveKey, NULLIFIER, SPEND));
        vm.expectRevert(Pool.NotEscrowed.selector);
        settleToAddress(signAddress(enclaveKey, NULLIFIER, SPEND));
    }

    function test_SettleRejectsNonPayingAmount() public {
        lock();
        vm.expectRevert(Pool.WrongSettlement.selector);
        pool.settleToAddress(
            proof,
            pc,
            pok,
            root,
            nullifiers(),
            outs(),
            ext(seller, 0),
            CALLS_ROOT,
            signAddress(enclaveKey, NULLIFIER, 0)
        );
    }

    // ---- settleToNote ---------------------------------------------------

    function test_SettleToNoteMovesNoTokens() public {
        lock();
        pool.settleToNote(
            proof,
            pc,
            pok,
            root,
            nullifiers(),
            outs(),
            ext(seller, 0),
            CALLS_ROOT,
            signNote(enclaveKey, NULLIFIER, outs()[0])
        );

        assertEq(token.balanceOf(seller), 0);
        assertEq(pool.leafCount(), 3);
        assertTrue(pool.nullifierSpent(NULLIFIER));
    }

    // The enclave signs the seller's commitment, so the amount inside it is
    // fixed without ever being public.
    function test_SettleToNoteRejectsOtherCommitment() public {
        lock();
        vm.expectRevert(Pool.WrongEnclave.selector);
        pool.settleToNote(
            proof,
            pc,
            pok,
            root,
            nullifiers(),
            outs(),
            ext(seller, 0),
            CALLS_ROOT,
            signNote(enclaveKey, NULLIFIER, 999)
        );
    }

    function test_SettleToNoteRejectsPublicAmount() public {
        lock();
        vm.expectRevert(Pool.WrongSettlement.selector);
        pool.settleToNote(
            proof,
            pc,
            pok,
            root,
            nullifiers(),
            outs(),
            ext(seller, -int256(SPEND)),
            CALLS_ROOT,
            signNote(enclaveKey, NULLIFIER, outs()[0])
        );
    }
}
