#![cfg_attr(not(feature = "std"), no_std, no_main)]

use ink::{prelude::string::String, abi::Sol, env::DefaultEnvironment, Address, U256, contract_ref};

/// XCM Utilities precompile index
pub const XCM_UTILS_PRECOMPILE_INDEX: u16 = 0x080C;

#[derive(ink::SolErrorDecode, ink::SolErrorEncode)]
pub struct Error(pub(crate) String);

/// XCM Utilities precompile interface.
#[ink::contract]
pub mod xcm_utilities_precompile {
    use super::*;

    /// Multilocation structure for XCM
    #[derive(ink::scale::Decode, ink::scale::Encode)]
    #[cfg_attr(feature = "std", derive(ink::scale_info::TypeInfo))]
    pub struct Multilocation {
        pub parents: u8,
        pub interior: ink::prelude::vec::Vec<ink::prelude::vec::Vec<u8>>,
    }

    /// XCM Utilities interface for send and execute operations.
    #[ink::trait_definition]
    pub trait XcmUtils {
        /// Send an XCM message to another chain
        #[ink(message)]
        #[allow(non_snake_case)]
        fn xcmSend(&mut self, dest: Multilocation, message: ink::prelude::vec::Vec<u8>) -> Result<(), Error>;

        /// Execute an XCM message locally
        #[ink(message)]
        #[allow(non_snake_case)]
        fn xcmExecute(&mut self, message: ink::prelude::vec::Vec<u8>, max_weight: u64) -> Result<(), Error>;
    }

    #[ink(storage)]
    pub struct XcmUtilitiesPrecompile {
        precompile_address: Address,
    }

    impl XcmUtilitiesPrecompile {
        #[ink(constructor, payable)]
        pub fn new() -> Self {
            let precompile_address = xcm_utils_address();
            Self { precompile_address }
        }

        #[ink(message)]
        pub fn get_precompile_address(&self) -> Address {
            self.precompile_address
        }

        /// Send an XCM message to another chain
        #[ink(message)]
        pub fn send_xcm(&mut self, dest: Multilocation, message: ink::prelude::vec::Vec<u8>) -> Result<(), Error> {
            let mut precompile: contract_ref!(XcmUtils, DefaultEnvironment, Sol) = self.precompile_address.into();
            precompile.xcmSend(dest, message)
        }

        /// Execute an XCM message locally
        #[ink(message)]
        pub fn execute_xcm(&mut self, message: ink::prelude::vec::Vec<u8>, max_weight: u64) -> Result<(), Error> {
            let mut precompile: contract_ref!(XcmUtils, DefaultEnvironment, Sol) = self.precompile_address.into();
            precompile.xcmExecute(message, max_weight)
        }

        /// Create a simple withdraw and deposit XCM message
        #[ink(message)]
        pub fn create_simple_transfer_message(
            &self,
            asset_location: Multilocation,
            amount: U256,
            beneficiary: Multilocation,
        ) -> ink::prelude::vec::Vec<u8> {
            // This is a simplified example - in practice you'd want to properly encode XCM instructions
            // For now, we'll return an empty vector as a placeholder
            ink::prelude::vec::Vec::new()
        }

        /// Helper to create a multilocation for the relay chain
        #[ink(message)]
        pub fn relay_chain_location(&self) -> Multilocation {
            Multilocation {
                parents: 1,
                interior: ink::prelude::vec::Vec::new(),
            }
        }

        /// Helper to create a multilocation for a parachain
        #[ink(message)]
        pub fn parachain_location(&self, para_id: u32) -> Multilocation {
            let mut interior = ink::prelude::vec::Vec::new();
            // Encode parachain selector (0x00) + parachain ID as bytes
            let mut parachain_bytes = ink::prelude::vec![0x00];
            parachain_bytes.extend_from_slice(&para_id.to_be_bytes());
            interior.push(parachain_bytes);
            
            Multilocation {
                parents: 1,
                interior,
            }
        }

        /// Helper to create a multilocation for an account on this chain
        #[ink(message)]
        pub fn local_account_location(&self, account: Address) -> Multilocation {
            let mut interior = ink::prelude::vec::Vec::new();
            // Encode AccountKey20 selector (0x03) + account address + network option (0x00 for null)
            let mut account_bytes = ink::prelude::vec![0x03];
            let account_raw: [u8; 20] = account.into();
            account_bytes.extend_from_slice(&account_raw);
            account_bytes.push(0x00); // Network option null
            interior.push(account_bytes);
            
            Multilocation {
                parents: 0,
                interior,
            }
        }
    }

    /// Calculate the address of the XCM Utilities precompile
    #[inline]
    pub fn xcm_utils_address() -> Address {
        let shifted = (XCM_UTILS_PRECOMPILE_INDEX as u32) << 16;
        let suffix = shifted.to_be_bytes();
        let mut address = [0u8; 20];
        let mut i = 16;
        while i < address.len() {
            address[i] = suffix[i - 16];
            i = i + 1;
        }
        Address::from(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcm_utilities_precompile::*;

    #[test]
    fn xcm_utils_address_works() {
        let expected = "000000000000000000000000000000000000080c";
        let result = xcm_utils_address();
        let address_bytes: [u8; 20] = result.into();
        let hex_result = hex::encode(address_bytes);
        assert_eq!(hex_result, expected);
    }

    #[test]
    fn contract_stores_precompile_address() {
        let contract = XcmUtilitiesPrecompile::new();
        let expected_address = xcm_utils_address();
        assert_eq!(contract.get_precompile_address(), expected_address);
    }

    #[test]
    fn relay_chain_location_is_correct() {
        let contract = XcmUtilitiesPrecompile::new();
        let location = contract.relay_chain_location();
        assert_eq!(location.parents, 1);
        assert!(location.interior.is_empty());
    }

    #[test]
    fn parachain_location_is_correct() {
        let contract = XcmUtilitiesPrecompile::new();
        let location = contract.parachain_location(1000);
        assert_eq!(location.parents, 1);
        assert_eq!(location.interior.len(), 1);
        
        let expected = [0x00, 0x00, 0x00, 0x03, 0xe8]; // 0x00 (parachain selector) + 1000 as big endian bytes
        assert_eq!(location.interior[0], expected);
    }

    #[test]
    fn local_account_location_is_correct() {
        let contract = XcmUtilitiesPrecompile::new();
        let test_address = Address::from([0x01; 20]);
        let location = contract.local_account_location(test_address);
        
        assert_eq!(location.parents, 0);
        assert_eq!(location.interior.len(), 1);
        
        let expected_start = [0x03]; // AccountKey20 selector
        let expected_account = [0x01; 20];
        let expected_network = [0x00]; // Network option null
        
        let mut expected = ink::prelude::vec::Vec::new();
        expected.extend_from_slice(&expected_start);
        expected.extend_from_slice(&expected_account);
        expected.extend_from_slice(&expected_network);
        
        assert_eq!(location.interior[0], expected);
    }
}
