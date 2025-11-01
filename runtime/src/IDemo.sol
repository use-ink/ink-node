// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @dev The on-chain address of the Demo precompile.
address constant DEMO_PRECOMPILE_ADDRESS = address(0xB0000);

/// @title Interface for the Demo precompile
/// @notice A simple precompile demonstrating basic functionality.
/// @dev Documentation:
/// @dev - ink! Contract calling this interface: https://github.com/use-ink/ink-examples/precompile-demo
interface IDemo {
    /// @notice Estimates the `Weight` required to execute a given XCM message.
    /// @param mode The value `0` causes the `echo` function to revert.
    /// @return If `mode > 0`, the input `message` is echoed back to the caller.
    function echo(uint8 mode, bytes message) external view returns (bytes);
}