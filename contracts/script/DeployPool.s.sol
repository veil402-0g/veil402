// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

import {Script, console} from "forge-std/Script.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {EnclaveRegistry} from "../src/EnclaveRegistry.sol";
import {Pool, IEnclaveRegistry, IVerifier} from "../src/Pool.sol";
import {Verifier} from "../src/Verifier.sol";

// Stands in for a real deployment's token. A pool is fixed to one token, so
// testing against anything else would not exercise the asset binding.
contract PoolToken is ERC20 {
    constructor(address holder) ERC20("Veil Test USD", "vUSD") {
        _mint(holder, 1_000_000_000);
    }
}

/// Deploys a pool and everything it links against.
///
/// `VERIFIER` reuses an already deployed verifier; leaving it unset deploys a
/// fresh one, which is what anvil wants.
contract DeployPool is Script {
    function run() external {
        address deployer = msg.sender;
        address existing = vm.envOr("VERIFIER", address(0));

        vm.startBroadcast();

        IVerifier verifier =
            existing == address(0) ? IVerifier(address(new Verifier())) : IVerifier(existing);
        PoolToken token = new PoolToken(deployer);
        EnclaveRegistry registry = new EnclaveRegistry(deployer);
        Pool pool = new Pool(IERC20(address(token)), verifier, IEnclaveRegistry(address(registry)));

        vm.stopBroadcast();

        console.log("chainId  ", block.chainid);
        console.log("verifier ", address(verifier));
        console.log("token    ", address(token));
        console.log("registry ", address(registry));
        console.log("pool     ", address(pool));
        console.log("assetId  ", pool.assetId());
        console.log("deployer ", deployer);
    }
}
