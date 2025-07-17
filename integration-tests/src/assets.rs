use frame_support::traits::fungible::Inspect as _;
use frame_support::{
	assert_noop,
	pallet_prelude::{Decode, Encode},
	traits::fungibles::{
		approvals::Inspect as _, metadata::Inspect as _, roles::Inspect as _, Inspect as _,
	},
};
use pallet_revive::precompiles::alloy::alloy_sol_types::SolValue;
use sp_io::hashing::twox_256;

use super::*;

const CONTRACT: &str = "contracts/assets/target/ink/assets.polkavm";
use IERC20::*;

pallet_revive::precompiles::alloy::sol!(
	#![sol(extra_derives(Debug, PartialEq))]
	"interfaces/IERC20.sol"
);

#[cfg_attr(feature = "std", derive(Debug, Encode))]
pub struct Created {
	pub id: AssetId,
}

#[test]
fn asset_id_works() {
	let token = 42;
	let endowment = 100 * UNIT;
	ExtBuilder::new()
		.with_assets(vec![(token, ALICE, false, 1)])
		.with_asset_balances(vec![(token, BOB, endowment)])
		.build()
		.execute_with(|| {
			let contract = Contract::new(&BOB, 0, token);

			// Test that asset_id returns the correct token ID
			assert_eq!(contract.asset_id(), token);
		});
}

#[test]
fn total_supply_works() {
	let token = 2;
	let endowment = 100 * UNIT;
	ExtBuilder::new()
		.with_assets(vec![(token, ALICE, false, 1)])
		.with_asset_balances(vec![(token, BOB, endowment)])
		.build()
		.execute_with(|| {
			let contract = Contract::new(&BOB, 0, token);

			assert!(Assets::asset_exists(token));

			// Tokens in circulation.
			assert_eq!(total_supply_precompile(token), Assets::total_supply(token).into());
			assert_eq!(contract.total_supply(), Assets::total_supply(token).into());
			assert_eq!(contract.total_supply(), endowment.into());

			// No tokens in circulation.
			let token = AssetId::MAX;
			assert_eq!(contract.total_supply(), Assets::total_supply(token).into());
			assert_eq!(contract.total_supply(), 0.into());
		});
}

// A simple, strongly typed wrapper for the contract.
struct Contract {
	address: H160,
	creator: AccountId,
}

/// Calculates the address of a precompile at index `n` and with some additional prefix.
#[inline]
pub fn fixed_address(n: u16) -> H160 {
	let shifted = (n as u32) << 16;

	let suffix = shifted.to_be_bytes();
	let mut address = [0u8; 20];
	let mut i = 16;
	while i < address.len() {
		address[i] = suffix[i - 16];
		i = i + 1;
	}
	H160::from(address)
}

/// Calculates the address of a precompile at index `n` and with some additional prefix.
#[inline]
pub fn prefixed_address(n: u16, prefix: u32) -> H160 {
	let address = fixed_address(n);
	let mut address_bytes: [u8; 20] = address.into();
	address_bytes[..4].copy_from_slice(&prefix.to_be_bytes());
	H160::from(address_bytes)
}

fn total_supply_precompile(token: AssetId) -> U256 {
	let call = totalSupplyCall {};
	U256::from_little_endian(call_precompile(&BOB, call, 0, token).as_le_slice())
}

fn call_precompile<T: SolCall>(
	origin: &AccountId,
	call: T,
	value: Balance,
	token: AssetId,
) -> T::Return {
	let origin = RuntimeOrigin::signed(origin.clone());
	let dest = prefixed_address(0x0120, token);
	// let dest = self.address.clone();
	let data = call.abi_encode();
	let result = bare_call(origin, dest, value, GAS_LIMIT, STORAGE_DEPOSIT_LIMIT, data)
		.expect("should work");

	match result.did_revert() {
		true => panic!("Contract call reverted: {:?}", String::from_utf8_lossy(&result.data)),
		false => T::abi_decode_returns(&result.data).expect("unable to decode success value"),
	}
}

impl Contract {
	// Create a new instance of the contract through on-chain instantiation (ink! style).
	fn new(origin: &AccountId, value: Balance, token: AssetId) -> Self {
		let data = [blake_selector("create").to_vec(), token.encode()].concat();
		let salt = twox_256(&value.to_le_bytes());

		let address =
			instantiate(RuntimeOrigin::signed(origin.clone()), CONTRACT, value, data, Some(salt));
		Self { address, creator: origin.clone() }
	}

	fn total_supply(&self) -> U256 {
		let call = totalSupplyCall {};
		U256::from_little_endian(self.call(&self.creator, call, 0).as_le_slice())
	}

	fn asset_id(&self) -> AssetId {
		let call = assetIdCall {};
		let data = self.call(&self.creator, call, 0);
		// let bytes: [u8; 4] = data.as_slice().try_into().expect("Expected 4 bytes for u32");
		// let result = u32::from_le_bytes(bytes);
		AssetId::from(data)
	}

	fn account_id(&self) -> AccountId {
		to_account_id(&self.address)
	}

	fn call<T: SolCall>(&self, origin: &AccountId, call: T, value: Balance) -> T::Return {
		let origin = RuntimeOrigin::signed(origin.clone());
		let dest = self.address.clone();
		let data = call.abi_encode();
		let result = bare_call(origin, dest, value, GAS_LIMIT, STORAGE_DEPOSIT_LIMIT, data)
			.expect("should work");

		match result.did_revert() {
			true => panic!("Contract call reverted: {:?}", String::from_utf8_lossy(&result.data)),
			false => T::abi_decode_returns(&result.data).expect("unable to decode success value"),
		}
	}

	fn last_event(&self) -> Vec<u8> {
		last_contract_event(&self.address)
	}
}
