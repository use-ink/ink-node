use frame_support::{
	assert_noop,
	pallet_prelude::Encode,
	traits::fungibles::{
		approvals::Inspect as _, metadata::Inspect as _, roles::Inspect as _, Inspect as _,
	},
};
use sp_io::hashing::twox_256;

use super::*;

const CONTRACT: &str = "contracts/assets/target/ink/assets.polkavm";
use IERC20::*;

pallet_revive::precompiles::alloy::sol!(
	#![sol(extra_derives(Debug, PartialEq))]
	"interfaces/IERC20.sol"
);

#[test]
fn total_supply_works() {
	let token = 1;
	let endowment = 100;
	ExtBuilder::new()
		.with_assets(vec![(token, ALICE, false, 1)])
		.with_asset_balances(vec![(token, BOB, endowment)])
		.build()
		.execute_with(|| {
			let contract = Contract::new(&BOB, 0, token);

			// Tokens in circulation.
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

impl Contract {
	// Create a new instance of the contract through on-chain instantiation.
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
