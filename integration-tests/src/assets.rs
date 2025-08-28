use frame_support::{
	pallet_prelude::Encode,
	traits::fungibles::Inspect as _,
};
use sp_io::hashing::twox_256;

use super::*;

const CONTRACT: &str = "contracts/assets/target/ink/assets.polkavm";
use IERC20::*;

// Import alloy primitives for type conversion
use pallet_revive::precompiles::alloy::primitives as alloy;

pallet_revive::precompiles::alloy::sol!(
	#![sol(extra_derives(Debug, PartialEq))]
	"interfaces/IERC20.sol"
);

#[cfg_attr(feature = "std", derive(Debug, Encode))]
pub struct Created {
	pub id: AssetId,
}

// Convert pallet_revive::U256 to alloy::U256 via byte conversion
fn to_alloy_u256(value: U256) -> alloy::U256 {
	// Convert to a 32-byte array and then to alloy::U256
	let mut bytes = [0u8; 32];
	for i in 0..4 {
		let word = value.0[i];
		bytes[i * 8..(i + 1) * 8].copy_from_slice(&word.to_le_bytes());
	}
	alloy::U256::from_le_slice(&bytes)
}

// Convert alloy::U256 to pallet_revive::U256 via byte conversion  
fn from_alloy_u256(value: alloy::U256) -> U256 {
	U256::from_little_endian(&value.to_le_bytes::<32>())
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

		});
}

#[test]
fn balance_of_works() {
	let token = 3;
	let endowment = 100 * UNIT;
	ExtBuilder::new()
		.with_assets(vec![(token, ALICE, false, 1)])
		.with_asset_balances(vec![(token, BOB, endowment), (token, CHARLIE, 50 * UNIT)])
		.build()
		.execute_with(|| {
			let contract = Contract::new(&BOB, 0, token);
			let bob_address = to_address(&BOB);
			let charlie_address = to_address(&CHARLIE);

			// Test contract balance_of
			assert_eq!(contract.balance_of(bob_address), endowment.into());
			assert_eq!(contract.balance_of(charlie_address), (50 * UNIT).into());

			// Test precompile balance_of
			assert_eq!(balance_of_precompile(token, bob_address), endowment.into());
			assert_eq!(balance_of_precompile(token, charlie_address), (50 * UNIT).into());

			// Test with account that has no balance
			let alice_address = to_address(&ALICE);
			assert_eq!(contract.balance_of(alice_address), U256::from(0));
			assert_eq!(balance_of_precompile(token, alice_address), U256::from(0));
		});
}

#[test]
fn allowance_works() {
	let token = 4;
	let endowment = 100 * UNIT;
	ExtBuilder::new()
		.with_assets(vec![(token, ALICE, false, 1)])
		.with_asset_balances(vec![(token, BOB, endowment)])
		.build()
		.execute_with(|| {
			let contract = Contract::new(&BOB, 0, token);
			let bob_address = to_address(&BOB);
			let charlie_address = to_address(&CHARLIE);

			// Initially no allowance
			assert_eq!(contract.allowance(bob_address, charlie_address), U256::from(0));
			assert_eq!(allowance_precompile(token, bob_address, charlie_address), U256::from(0));
		});
}

#[test]
fn approve_works() {
	let token = 5;
	let endowment = 100 * UNIT;
	let approve_amount = 25 * UNIT;
	ExtBuilder::new()
		.with_assets(vec![(token, ALICE, false, 1)])
		.with_asset_balances(vec![(token, BOB, endowment)])
		.build()
		.execute_with(|| {
			let mut contract = Contract::new(&BOB, 0, token);
			let bob_address = to_address(&BOB);
			let charlie_address = to_address(&CHARLIE);

			// Check initial allowance
			assert_eq!(contract.allowance(bob_address, charlie_address), U256::from(0));

			// Test just contract approve
			assert!(contract.approve(&BOB, charlie_address, approve_amount.into()).unwrap());

			// Check allowance after approve
			assert_eq!(contract.allowance(bob_address, charlie_address), approve_amount.into());
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

// Precompile function wrappers - read-only functions
fn total_supply_precompile(token: AssetId) -> U256 {
	let call = totalSupplyCall {};
	U256::from_little_endian(call_precompile(&BOB, call, 0, token).as_le_slice())
}

fn balance_of_precompile(token: AssetId, account: H160) -> U256 {
	let account_bytes: [u8; 20] = account.into();
	let call = balanceOfCall { account: account_bytes.into() };
	U256::from_little_endian(call_precompile(&BOB, call, 0, token).as_le_slice())
}

fn allowance_precompile(token: AssetId, owner: H160, spender: H160) -> U256 {
	let owner_bytes: [u8; 20] = owner.into();
	let spender_bytes: [u8; 20] = spender.into();
	let call = allowanceCall { owner: owner_bytes.into(), spender: spender_bytes.into() };
	U256::from_little_endian(call_precompile(&BOB, call, 0, token).as_le_slice())
}

// Precompile function wrappers - write functions
fn transfer_precompile(origin: &AccountId, token: AssetId, to: H160, value: U256) -> bool {
	let to_bytes: [u8; 20] = to.into();
	let alloy_value = to_alloy_u256(value);
	let call = transferCall { to: to_bytes.into(), value: alloy_value };
	call_precompile(origin, call, 0, token)
}

fn approve_precompile(origin: &AccountId, token: AssetId, spender: H160, value: U256) -> bool {
	let spender_bytes: [u8; 20] = spender.into();
	let alloy_value = to_alloy_u256(value);
	let call = approveCall { spender: spender_bytes.into(), value: alloy_value };
	call_precompile(origin, call, 0, token)
}

fn transfer_from_precompile(origin: &AccountId, token: AssetId, from: H160, to: H160, value: U256) -> bool {
	let from_bytes: [u8; 20] = from.into();
	let to_bytes: [u8; 20] = to.into();
	let alloy_value = to_alloy_u256(value);
	let call = transferFromCall { from: from_bytes.into(), to: to_bytes.into(), value: alloy_value };
	call_precompile(origin, call, 0, token)
}

fn call_precompile<T: SolCall>(
	origin: &AccountId,
	call: T,
	value: Balance,
	token: AssetId,
) -> T::Return {
	let origin = RuntimeOrigin::signed(origin.clone());
	let dest = prefixed_address(0x0120, token);
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
		use pallet_revive::precompiles::alloy::sol_types::SolValue;
		let data = (token,).abi_encode();
		let salt = twox_256(&value.to_le_bytes());

		let address =
			instantiate(RuntimeOrigin::signed(origin.clone()), CONTRACT, value, data, Some(salt));
		Self { address, creator: origin.clone() }
	}

	fn total_supply(&self) -> U256 {
		let call = totalSupplyCall {};
		U256::from_little_endian(self.call(&self.creator, call, 0).as_le_slice())
	}

	fn balance_of(&self, account: H160) -> U256 {
		let account_bytes: [u8; 20] = account.into();
		let call = balanceOfCall { account: account_bytes.into() };
		U256::from_little_endian(self.call(&self.creator, call, 0).as_le_slice())
	}

	fn allowance(&self, owner: H160, spender: H160) -> U256 {
		let owner_bytes: [u8; 20] = owner.into();
		let spender_bytes: [u8; 20] = spender.into();
		let call = allowanceCall { owner: owner_bytes.into(), spender: spender_bytes.into() };
		U256::from_little_endian(self.call(&self.creator, call, 0).as_le_slice())
	}

	fn transfer(&mut self, origin: &AccountId, to: H160, value: U256) -> Result<bool, String> {
		let to_bytes: [u8; 20] = to.into();
		let alloy_value = to_alloy_u256(value);
		let call = transferCall { to: to_bytes.into(), value: alloy_value };
		Ok(self.call(origin, call, 0))
	}

	fn approve(&mut self, origin: &AccountId, spender: H160, value: U256) -> Result<bool, String> {
		let spender_bytes: [u8; 20] = spender.into();
		let alloy_value = to_alloy_u256(value);
		let call = approveCall { spender: spender_bytes.into(), value: alloy_value };
		Ok(self.call(origin, call, 0))
	}

	fn transfer_from(&mut self, origin: &AccountId, from: H160, to: H160, value: U256) -> Result<bool, String> {
		let from_bytes: [u8; 20] = from.into();
		let to_bytes: [u8; 20] = to.into();
		let alloy_value = to_alloy_u256(value);
		let call = transferFromCall { from: from_bytes.into(), to: to_bytes.into(), value: alloy_value };
		Ok(self.call(origin, call, 0))
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
}