// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {ECDSA} from "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";
import {PoseidonT3} from "poseidon-solidity/PoseidonT3.sol";
import {PoseidonT5} from "poseidon-solidity/PoseidonT5.sol";
import {InternalLazyIMT, LazyIMTData} from "@zk-kit/lazy-imt/InternalLazyIMT.sol";
import {SNARK_SCALAR_FIELD} from "@zk-kit/lazy-imt/Constants.sol";

interface IEnclaveRegistry {
    function isLive(address signer) external view returns (bool);
}

interface IVerifier {
    function verifyProof(
        uint256[8] calldata proof,
        uint256[2] calldata commitments,
        uint256[2] calldata commitmentPok,
        uint256[7] calldata input
    ) external view;
}

/// Shielded pool over a single ERC20. Deposits are public and enter through
/// `shield`; every later movement is a join-split proved against the tree.
///
/// Amounts are bounded to 64 bits because the circuit range-checks note values.
/// Fee-on-transfer and rebasing tokens are unsupported.
contract Pool is ReentrancyGuard, EIP712 {
    using SafeERC20 for IERC20;
    using InternalLazyIMT for LazyIMTData;

    uint8 public constant TREE_DEPTH = 20;
    uint256 public constant MAX_NOTE_BYTES = 128;

    /// Longest an escrow may hold a note. Bounds how long a front-run lock can
    /// strand someone else's note.
    uint64 public constant MAX_LOCK = 1 hours;
    /// The seller settles up to the deadline; the buyer regains the note only
    /// after this, so the two can never race for the same instant.
    uint64 public constant SETTLE_GRACE = 60;

    bytes32 private constant LOCK_TYPEHASH =
        keccak256("Lock(uint256 nullifier,address seller,uint64 deadline)");
    bytes32 private constant SETTLE_TO_ADDRESS_TYPEHASH =
        keccak256("Settlement(uint256 nullifier,uint256 amount,bytes32 callsRoot)");
    bytes32 private constant SETTLE_TO_NOTE_TYPEHASH =
        keccak256("NoteSettlement(uint256 nullifier,uint256 sellerCommitment,bytes32 callsRoot)");

    uint256 private constant NOTE_DOMAIN = 5;
    uint256 private constant ASSET_DOMAIN = 7;
    bytes32 private constant BINDING_DOMAIN = keccak256("veil402:transact:v5");

    /// A note reserved against a seller until its deadline. The pool holds no
    /// value for it: the note stays the buyer's, it just cannot be spent
    /// elsewhere while the reservation is live.
    struct Escrow {
        address enclave;
        address seller;
        uint64 deadline;
        bool settled;
    }

    /// Recipient, signed public amount and output ciphertexts, bound into the
    /// proof so a relayer cannot redirect or re-price a withdrawal.
    struct ExtData {
        address recipient;
        int256 extAmount;
        bytes[2] notes;
    }

    IERC20 public immutable token;
    IVerifier public immutable verifier;
    IEnclaveRegistry public immutable registry;
    uint256 public immutable assetId;

    LazyIMTData private tree;
    mapping(uint256 => bool) public knownRoot;
    mapping(uint256 => bool) public nullifierSpent;
    mapping(uint256 => Escrow) public escrows;

    event NewCommitment(uint256 indexed commitment, uint256 leafIndex, bytes note);
    event Nullified(uint256 indexed nullifier);
    event Locked(
        uint256 indexed nullifier, address indexed enclave, address indexed seller, uint64 deadline
    );
    event Settled(uint256 indexed nullifier, bytes32 callsRoot);

    error AlreadyLocked();
    error AmountOutOfRange();
    error BadDeadline();
    error DepositMustUseShield();
    error DuplicateNullifier();
    error EnclaveNotLive();
    error EscrowExpired();
    error InvalidRecipient();
    error NotEscrowed();
    error NotInField();
    error NoteTooLarge();
    error NullifierAlreadySpent();
    error NullifierReserved();
    error UnknownRoot();
    error WrongEnclave();
    error WrongSettlement();
    error ZeroAddress();
    error ZeroAmount();
    error ZeroNullifier();

    constructor(IERC20 _token, IVerifier _verifier, IEnclaveRegistry _registry)
        EIP712("Veil402Pool", "1")
    {
        if (
            address(_token) == address(0) || address(_verifier) == address(0)
                || address(_registry) == address(0)
        ) revert ZeroAddress();
        token = _token;
        verifier = _verifier;
        registry = _registry;
        assetId = PoseidonT3.hash([ASSET_DOMAIN, uint256(uint160(address(_token)))]);
        tree._init(TREE_DEPTH);
    }

    /// Deposits `amount` publicly and commits it to `npk`.
    function shield(uint256 npk, uint64 amount, bytes calldata note) external nonReentrant {
        if (amount == 0) revert ZeroAmount();
        if (npk == 0 || npk >= SNARK_SCALAR_FIELD) revert NotInField();
        if (note.length > MAX_NOTE_BYTES) revert NoteTooLarge();

        uint256 commitment = PoseidonT5.hash([NOTE_DOMAIN, npk, assetId, amount]);
        _append(commitment, note);
        _recordRoot();
        token.safeTransferFrom(msg.sender, address(this), amount);
    }

    /// Spends two notes and creates two. A negative `extAmount` leaves the pool
    /// to `recipient`; zero keeps the value shielded.
    function transact(
        uint256[8] calldata proof,
        uint256[2] calldata proofCommitments,
        uint256[2] calldata proofCommitmentPok,
        uint256 root,
        uint256[2] calldata nullifiers,
        uint256[2] calldata outCommitments,
        ExtData calldata ext
    ) external nonReentrant {
        for (uint256 i = 0; i < 2; i++) {
            if (_reserved(nullifiers[i])) revert NullifierReserved();
        }
        _spend(proof, proofCommitments, proofCommitmentPok, root, nullifiers, outCommitments, ext);
    }

    /// Reserves `nullifier` against `seller` until `deadline`, so the note it
    /// spends cannot go anywhere else while the session runs.
    ///
    /// Only the note's owner can name its nullifier, and the enclave must have
    /// agreed to meter the session, so a reservation is attributable to a key
    /// buyers pin and we can revoke.
    function lock(
        uint256 nullifier,
        address enclave,
        address seller,
        uint64 deadline,
        bytes calldata signature
    ) external {
        if (nullifier == 0) revert ZeroNullifier();
        if (nullifier >= SNARK_SCALAR_FIELD) revert NotInField();
        if (nullifierSpent[nullifier]) revert NullifierAlreadySpent();
        if (escrows[nullifier].enclave != address(0)) revert AlreadyLocked();
        if (enclave == address(0) || seller == address(0) || seller == address(this)) {
            revert InvalidRecipient();
        }
        if (deadline <= block.timestamp || deadline > block.timestamp + MAX_LOCK) {
            revert BadDeadline();
        }
        if (!registry.isLive(enclave)) revert EnclaveNotLive();

        bytes32 structHash = keccak256(abi.encode(LOCK_TYPEHASH, nullifier, seller, deadline));
        if (ECDSA.recover(_hashTypedDataV4(structHash), signature) != enclave) {
            revert WrongEnclave();
        }

        escrows[nullifier] = Escrow(enclave, seller, deadline, false);
        emit Locked(nullifier, enclave, seller, deadline);
    }

    /// Settles an escrow by paying the seller publicly. The ceiling and the
    /// buyer's change both stay hidden; only the amount paid is visible.
    function settleToAddress(
        uint256[8] calldata proof,
        uint256[2] calldata proofCommitments,
        uint256[2] calldata proofCommitmentPok,
        uint256 root,
        uint256[2] calldata nullifiers,
        uint256[2] calldata outCommitments,
        ExtData calldata ext,
        bytes32 callsRoot,
        bytes calldata signature
    ) external nonReentrant {
        uint256 nullifier = _open(nullifiers);
        if (ext.extAmount >= 0) revert WrongSettlement();

        address seller = _authorize(
            nullifier,
            keccak256(
                abi.encode(
                    SETTLE_TO_ADDRESS_TYPEHASH, nullifier, uint256(-ext.extAmount), callsRoot
                )
            ),
            callsRoot,
            signature
        );
        if (ext.recipient != seller) revert InvalidRecipient();
        _spend(proof, proofCommitments, proofCommitmentPok, root, nullifiers, outCommitments, ext);
    }

    /// Settles an escrow by paying the seller a note instead of tokens, so the
    /// amount never becomes public. The seller needs note discovery to spend it.
    function settleToNote(
        uint256[8] calldata proof,
        uint256[2] calldata proofCommitments,
        uint256[2] calldata proofCommitmentPok,
        uint256 root,
        uint256[2] calldata nullifiers,
        uint256[2] calldata outCommitments,
        ExtData calldata ext,
        bytes32 callsRoot,
        bytes calldata signature
    ) external nonReentrant {
        uint256 nullifier = _open(nullifiers);
        if (ext.extAmount != 0) revert WrongSettlement();

        _authorize(
            nullifier,
            keccak256(abi.encode(SETTLE_TO_NOTE_TYPEHASH, nullifier, outCommitments[0], callsRoot)),
            callsRoot,
            signature
        );
        _spend(proof, proofCommitments, proofCommitmentPok, root, nullifiers, outCommitments, ext);
    }

    /// Returns the live escrowed nullifier of the pair, or reverts.
    ///
    /// Both being reserved would let one settlement consume the other escrow's
    /// note without its enclave ever authorising it.
    function _open(uint256[2] calldata nullifiers) private view returns (uint256) {
        bool first = _reserved(nullifiers[0]);
        bool second = _reserved(nullifiers[1]);
        if (first && second) revert NullifierReserved();
        if (first) return nullifiers[0];
        if (second) return nullifiers[1];
        revert NotEscrowed();
    }

    function _authorize(
        uint256 nullifier,
        bytes32 structHash,
        bytes32 callsRoot,
        bytes calldata signature
    ) private returns (address seller) {
        Escrow storage escrow = escrows[nullifier];
        if (block.timestamp > escrow.deadline) revert EscrowExpired();
        if (ECDSA.recover(_hashTypedDataV4(structHash), signature) != escrow.enclave) {
            revert WrongEnclave();
        }
        escrow.settled = true;
        emit Settled(nullifier, callsRoot);
        return escrow.seller;
    }

    /// A reservation outlives its deadline by the grace window, so a settlement
    /// landing late cannot collide with the buyer taking the note back.
    function _reserved(uint256 nullifier) private view returns (bool) {
        Escrow storage escrow = escrows[nullifier];
        return escrow.enclave != address(0) && !escrow.settled
            && block.timestamp <= uint256(escrow.deadline) + SETTLE_GRACE;
    }

    function _spend(
        uint256[8] calldata proof,
        uint256[2] calldata proofCommitments,
        uint256[2] calldata proofCommitmentPok,
        uint256 root,
        uint256[2] calldata nullifiers,
        uint256[2] calldata outCommitments,
        ExtData calldata ext
    ) private {
        if (ext.extAmount > 0) revert DepositMustUseShield();
        if (ext.extAmount < type(int64).min) revert AmountOutOfRange();
        uint256 withdrawal = uint256(-ext.extAmount);

        if (root == 0 || root >= SNARK_SCALAR_FIELD) revert NotInField();
        if (!knownRoot[root]) revert UnknownRoot();
        if (nullifiers[0] == nullifiers[1]) revert DuplicateNullifier();
        if (ext.recipient == address(this)) revert InvalidRecipient();
        if (withdrawal != 0 && ext.recipient == address(0)) revert InvalidRecipient();

        for (uint256 i = 0; i < 2; i++) {
            if (nullifiers[i] == 0) revert ZeroNullifier();
            if (nullifiers[i] >= SNARK_SCALAR_FIELD) revert NotInField();
            if (outCommitments[i] >= SNARK_SCALAR_FIELD) revert NotInField();
            if (nullifierSpent[nullifiers[i]]) revert NullifierAlreadySpent();
        }

        verifier.verifyProof(
            proof,
            proofCommitments,
            proofCommitmentPok,
            [
                root,
                nullifiers[0],
                nullifiers[1],
                outCommitments[0],
                outCommitments[1],
                publicAmount(ext.extAmount),
                extDataHash(ext)
            ]
        );

        for (uint256 i = 0; i < 2; i++) {
            nullifierSpent[nullifiers[i]] = true;
            emit Nullified(nullifiers[i]);
        }
        _append(outCommitments[0], ext.notes[0]);
        _append(outCommitments[1], ext.notes[1]);
        _recordRoot();

        if (withdrawal != 0) token.safeTransfer(ext.recipient, withdrawal);
    }

    function currentRoot() external view returns (uint256) {
        return tree._root(TREE_DEPTH);
    }

    function leafCount() external view returns (uint256) {
        return tree.numberOfLeaves;
    }

    /// Binds the proof to this pool on this chain. Ciphertexts are
    /// length-prefixed so bytes cannot be shifted from one note into the other.
    function extDataHash(ExtData calldata ext) public view returns (uint256) {
        if (ext.notes[0].length > MAX_NOTE_BYTES || ext.notes[1].length > MAX_NOTE_BYTES) {
            revert NoteTooLarge();
        }
        bytes32 notes = keccak256(
            abi.encodePacked(
                uint32(ext.notes[0].length), ext.notes[0], uint32(ext.notes[1].length), ext.notes[1]
            )
        );
        return uint256(
            keccak256(
                abi.encode(
                    BINDING_DOMAIN,
                    block.chainid,
                    address(this),
                    address(token),
                    address(verifier),
                    ext.recipient,
                    ext.extAmount,
                    notes
                )
            )
        ) % SNARK_SCALAR_FIELD;
    }

    /// The circuit reads the public amount as a field element, so a withdrawal
    /// is the additive inverse rather than a sign bit.
    function publicAmount(int256 extAmount) public pure returns (uint256) {
        return extAmount < 0 ? SNARK_SCALAR_FIELD - uint256(-extAmount) : uint256(extAmount);
    }

    function _append(uint256 commitment, bytes calldata note) private {
        uint256 leafIndex = tree.numberOfLeaves;
        tree._insert(commitment);
        emit NewCommitment(commitment, leafIndex, note);
    }

    // Only transaction boundaries are provable states, so the root is derived
    // and recorded once per call rather than once per leaf.
    function _recordRoot() private {
        knownRoot[tree._root(TREE_DEPTH)] = true;
    }
}
