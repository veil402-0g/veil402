// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {PoseidonT3} from "poseidon-solidity/PoseidonT3.sol";
import {PoseidonT5} from "poseidon-solidity/PoseidonT5.sol";
import {InternalLazyIMT, LazyIMTData} from "@zk-kit/lazy-imt/InternalLazyIMT.sol";
import {SNARK_SCALAR_FIELD} from "@zk-kit/lazy-imt/Constants.sol";

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
contract Pool is ReentrancyGuard {
    using SafeERC20 for IERC20;
    using InternalLazyIMT for LazyIMTData;

    uint8 public constant TREE_DEPTH = 20;
    uint256 public constant MAX_NOTE_BYTES = 128;

    uint256 private constant NOTE_DOMAIN = 5;
    uint256 private constant ASSET_DOMAIN = 7;
    bytes32 private constant BINDING_DOMAIN = keccak256("veil402:transact:v5");

    /// Recipient, signed public amount and output ciphertexts, bound into the
    /// proof so a relayer cannot redirect or re-price a withdrawal.
    struct ExtData {
        address recipient;
        int256 extAmount;
        bytes[2] notes;
    }

    IERC20 public immutable token;
    IVerifier public immutable verifier;
    uint256 public immutable assetId;

    LazyIMTData private tree;
    mapping(uint256 => bool) public knownRoot;
    mapping(uint256 => bool) public nullifierSpent;

    event NewCommitment(uint256 indexed commitment, uint256 leafIndex, bytes note);
    event Nullified(uint256 indexed nullifier);

    error AmountOutOfRange();
    error DepositMustUseShield();
    error DuplicateNullifier();
    error InvalidRecipient();
    error NoteTooLarge();
    error NotInField();
    error NullifierAlreadySpent();
    error UnknownRoot();
    error ZeroAmount();
    error ZeroNullifier();

    constructor(IERC20 _token, IVerifier _verifier) {
        token = _token;
        verifier = _verifier;
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
