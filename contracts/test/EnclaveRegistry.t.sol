// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {EnclaveRegistry} from "../src/EnclaveRegistry.sol";

contract EnclaveRegistryTest is Test {
    EnclaveRegistry registry;

    address owner = address(0xB0551);
    address signer = address(0x51611E2);
    bytes32 image = keccak256("PCR0");

    function setUp() public {
        registry = new EnclaveRegistry(owner);
    }

    function test_RegistersAndRevokes() public {
        vm.prank(owner);
        registry.register(signer, image);
        assertTrue(registry.isLive(signer));
        (bytes32 recorded,, uint64 revokedAt) = registry.enclaves(signer);
        assertEq(recorded, image);
        assertEq(revokedAt, 0);

        vm.prank(owner);
        registry.revoke(signer);
        assertFalse(registry.isLive(signer));
    }

    function test_RejectsBadInput() public {
        vm.startPrank(owner);
        vm.expectRevert(EnclaveRegistry.ZeroSigner.selector);
        registry.register(address(0), image);

        vm.expectRevert(EnclaveRegistry.ZeroImage.selector);
        registry.register(signer, bytes32(0));

        vm.expectRevert(EnclaveRegistry.NotRegistered.selector);
        registry.revoke(signer);

        // A key is bound to one image for life; rebinding would make an
        // earlier pin meaningless.
        registry.register(signer, image);
        vm.expectRevert(EnclaveRegistry.AlreadyRegistered.selector);
        registry.register(signer, keccak256("other"));
        vm.stopPrank();
    }

    function test_OnlyOwnerMayWrite() public {
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, address(this)));
        registry.register(signer, image);
    }

    function test_UnknownSignerIsNotLive() public view {
        assertFalse(registry.isLive(signer));
    }
}
