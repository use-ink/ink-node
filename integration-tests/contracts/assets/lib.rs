#![cfg_attr(not(feature = "std"), no_std, no_main)]

use ink::{prelude::string::String, abi::Sol, env::DefaultEnvironment, Address, U256, contract_ref};

/// Passet Hub ERC20 precompile index
pub const PRECOMPILE_INDEX: u16 = 0x0120;
pub type AssetId = u32;

#[derive(ink::SolErrorDecode, ink::SolErrorEncode)]
pub struct Error(pub(crate) String);

/// Asset Hub precompile interface.
#[ink::contract]
pub mod asset_hub_precompile {
    use super::*;

    /// Simple ERC-20 interface - just the basics.
    #[ink::trait_definition]
    pub trait Erc20 {
        #[ink(message)]
        #[allow(non_snake_case)]
        fn totalSupply(&self) -> U256;

        #[ink(message)]
        #[allow(non_snake_case)]
        fn balanceOf(&self, account: Address) -> U256;

        #[ink(message)]
        fn transfer(&mut self, to: Address, value: U256) -> Result<bool, Error>;

        #[ink(message)]
        fn allowance(&self, owner: Address, spender: Address) -> U256;

        #[ink(message)]
        fn approve(&mut self, spender: Address, value: U256) -> Result<bool, Error>;

        #[ink(message)]
        #[allow(non_snake_case)]
        fn transferFrom(&mut self, from: Address, to: Address, value: U256) -> Result<bool, Error>;
    }

    #[ink(storage)]
    pub struct AssetHubPrecompile {
        asset_id: AssetId,
    }

    impl AssetHubPrecompile {
        #[ink(constructor, payable)]
        pub fn create(asset_id: AssetId) -> Self {
            Self { asset_id }
        }

        #[ink(message)]
        #[allow(non_snake_case)]
        pub fn assetId(&self) -> AssetId {
            self.asset_id
        }
    }

    impl Erc20 for AssetHubPrecompile {
        #[ink(message)]
        fn totalSupply(&self) -> U256 {
            let precompile_address = prefixed_address(PRECOMPILE_INDEX, self.asset_id);
            let precompile: contract_ref!(Erc20, DefaultEnvironment, Sol) = precompile_address.into();
            precompile.totalSupply()
        }

        #[ink(message)]
        fn balanceOf(&self, account: Address) -> U256 {
            let precompile_address = prefixed_address(PRECOMPILE_INDEX, self.asset_id);
            let precompile: contract_ref!(Erc20, DefaultEnvironment, Sol) = precompile_address.into();
            precompile.balanceOf(account)
        }

        #[ink(message)]
        fn transfer(&mut self, to: Address, amount: U256) -> Result<bool, Error> {
            let precompile_address = prefixed_address(PRECOMPILE_INDEX, self.asset_id);
            let mut precompile: contract_ref!(Erc20, DefaultEnvironment, Sol) = precompile_address.into();
            precompile.transfer(to, amount)
        }

        #[ink(message)]
        fn approve(&mut self, spender: Address, amount: U256) -> Result<bool, Error> {
            let precompile_address = prefixed_address(PRECOMPILE_INDEX, self.asset_id);
            let mut precompile: contract_ref!(Erc20, DefaultEnvironment, Sol) = precompile_address.into();
            precompile.approve(spender, amount)
        }

        #[ink(message)]
        fn allowance(&self, owner: Address, spender: Address) -> U256 {
            let precompile_address = prefixed_address(PRECOMPILE_INDEX, self.asset_id);
            let precompile: contract_ref!(Erc20, DefaultEnvironment, Sol) = precompile_address.into();
            precompile.allowance(owner, spender)
        }

        #[ink(message)]
        fn transferFrom(&mut self, from: Address, to: Address, amount: U256) -> Result<bool, Error> {
            let precompile_address = prefixed_address(PRECOMPILE_INDEX, self.asset_id);
            let mut precompile: contract_ref!(Erc20, DefaultEnvironment, Sol) = precompile_address.into();
            precompile.transferFrom(from, to, amount)
        }
    }

    /// Calculates the address of a precompile at index `n`.
    #[inline]
    pub fn fixed_address(n: u16) -> Address {
        let shifted = (n as u32) << 16;

        let suffix = shifted.to_be_bytes();
        let mut address = [0u8; 20];
        let mut i = 16;
        while i < address.len() {
            address[i] = suffix[i - 16];
            i = i + 1;
        }
        Address::from(address)
    }

    /// Calculates the address of a precompile at index `n` and with some additional prefix.
    #[inline]
    pub fn prefixed_address(n: u16, prefix: u32) -> Address {
        let address = fixed_address(n);
        let mut address_bytes: [u8; 20] = address.into();
        address_bytes[..4].copy_from_slice(&prefix.to_be_bytes());
        Address::from(address_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_address_works() {
        let expected = "0000000000000000000000000000000000640000";
        let result = fixed_address(100);
        let address_bytes: [u8; 20] = result.into();
        let hex_result = hex::encode(address_bytes);
        assert_eq!(hex_result, expected);
    }

    #[test]
    fn prefixed_address_works() {
        let expected = "ffffffff00000000000000000000000000650000";
        let result = prefixed_address(101, u32::MAX);
        let address_bytes: [u8; 20] = result.into();
        let hex_result = hex::encode(address_bytes);
        assert_eq!(hex_result, expected);
    }

    #[test]
    fn contract_stores_asset_id() {
        use asset_hub_precompile::AssetHubPrecompile;
        
        // Create contract for asset ID 1337
        let contract = AssetHubPrecompile::new(1337);
        
        // Verify the asset ID is stored correctly
        assert_eq!(contract.asset_id(), 1337);
    }
} 