// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Ownable2Step, Ownable} from "@openzeppelin/contracts/access/Ownable2Step.sol";

/// Public record of which signing key belongs to which enclave image.
///
/// A buyer pins an enclave address directly when locking, so the pool never
/// reads this contract. It exists so that pinning is auditable: anyone can
/// check afterwards which `PCR0` a key claimed, and when it was revoked.
contract EnclaveRegistry is Ownable2Step {
    struct Enclave {
        bytes32 imageHash;
        uint64 registeredAt;
        uint64 revokedAt;
    }

    mapping(address => Enclave) public enclaves;

    event Registered(address indexed signer, bytes32 indexed imageHash);
    event Revoked(address indexed signer, bytes32 indexed imageHash);

    error AlreadyRegistered();
    error NotRegistered();
    error ZeroImage();
    error ZeroSigner();

    constructor(address owner) Ownable(owner) {}

    /// Binds a signing key to `keccak256(PCR0)` of the image that holds it.
    ///
    /// The attestation document is verified off chain against the AWS Nitro
    /// root; only its conclusion is recorded here.
    function register(address signer, bytes32 imageHash) external onlyOwner {
        if (signer == address(0)) revert ZeroSigner();
        if (imageHash == bytes32(0)) revert ZeroImage();
        if (enclaves[signer].registeredAt != 0) revert AlreadyRegistered();

        enclaves[signer] = Enclave(imageHash, uint64(block.timestamp), 0);
        emit Registered(signer, imageHash);
    }

    /// Marks a key as no longer trusted. Escrows already locked to it are
    /// unaffected — the buyer chose it, and only they bear that choice.
    function revoke(address signer) external onlyOwner {
        Enclave storage enclave = enclaves[signer];
        if (enclave.registeredAt == 0 || enclave.revokedAt != 0) revert NotRegistered();

        enclave.revokedAt = uint64(block.timestamp);
        emit Revoked(signer, enclave.imageHash);
    }

    function isLive(address signer) external view returns (bool) {
        Enclave storage enclave = enclaves[signer];
        return enclave.registeredAt != 0 && enclave.revokedAt == 0;
    }
}
