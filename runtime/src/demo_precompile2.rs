// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use core::{marker::PhantomData, num::NonZero};
use pallet_revive::precompiles::{
	alloy::{self, sol_types::SolValue},
	AddressMatcher, Error, Ext, Precompile,
};

alloy::sol!("src/IDemo.sol");
use IDemo::IDemoCalls;

const LOG_TARGET: &str = "custom::precompile";

pub struct DemoPrecompile<T>(PhantomData<T>);

impl<Runtime> Precompile for DemoPrecompile<Runtime>
where
	Runtime: pallet_revive::Config,
{
	type T = Runtime;
	const MATCHER: AddressMatcher = AddressMatcher::Fixed(NonZero::new(12).unwrap());
	const HAS_CONTRACT_INFO: bool = true;
	type Interface = IDemo2::IDemoCalls;

	fn call(
		_address: &[u8; 20],
		input: &Self::Interface,
		env: &mut impl Ext<T = Self::T>,
	) -> Result<(), Error> {
		let origin = env.caller();
		log::debug!(target: LOG_TARGET, "got a call from origin {origin:?}");

		match input {
			IDemo2Calls::addOne(IDemo2::addOneCall { }) => {
				log::debug!(target: LOG_TARGET, "adding one");
				Ok(())
			},
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use frame_support::{derive_impl, parameter_types, weights::Weight};
	use pallet_revive::{
		precompiles::{
			alloy::{
				hex,
				primitives::Bytes,
				sol_types::{SolInterface, SolValue},
			},
			H160,
		},
		ExecConfig, U256,
	};
	use sp_runtime::{traits::IdentityLookup, AccountId32, BuildStorage};

	pub type AccountId = AccountId32;
	pub type Balance = u128;
	type Block = frame_system::mocking::MockBlock<Test>;

	pub const ALICE: AccountId32 = AccountId::new([0u8; 32]);
	const CUSTOM_INITIAL_BALANCE: u128 = 100_000_000_000u128;

	parameter_types! {
		pub const MinimumPeriod: u64 = 1;
	}

	impl pallet_timestamp::Config for Test {
		type Moment = u64;
		type OnTimestampSet = ();
		type MinimumPeriod = MinimumPeriod;
		type WeightInfo = ();
	}

	frame_support::construct_runtime!(
		pub enum Test
		{
			System: frame_system,
			Balances: pallet_balances,
			Revive: pallet_revive,
			Timestamp: pallet_timestamp,
		}
	);

	#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
	impl frame_system::Config for Test {
		type AccountId = AccountId;
		type Lookup = IdentityLookup<Self::AccountId>;
		type Block = Block;
		type AccountData = pallet_balances::AccountData<Balance>;
	}

	parameter_types! {
		pub ExistentialDeposit: Balance = 1;
	}

	#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
	impl pallet_balances::Config for Test {
		type Balance = Balance;
		type ExistentialDeposit = ExistentialDeposit;
		type AccountStore = System;
	}

	#[derive_impl(pallet_revive::config_preludes::TestDefaultConfig)]
	impl pallet_revive::Config for Test {
		type AddressMapper = pallet_revive::AccountId32Mapper<Self>;
		type Balance = Balance;
		type Currency = Balances;
		type Precompiles = (DemoPrecompile<Self>,);
		type Time = Timestamp;
		type UploadOrigin = frame_system::EnsureSigned<AccountId>;
		type InstantiateOrigin = frame_system::EnsureSigned<AccountId>;
	}

	pub fn new_test_ext_with_balances(
		balances: Vec<(AccountId, Balance)>,
	) -> sp_io::TestExternalities {
		let mut t = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();

		pallet_balances::GenesisConfig::<Test> { balances, ..Default::default() }
			.assimilate_storage(&mut t)
			.unwrap();

		pallet_revive::GenesisConfig::<Test> { mapped_accounts: vec![ALICE], ..Default::default() }
			.assimilate_storage(&mut t)
			.unwrap();

		let mut ext = sp_io::TestExternalities::new(t);
		ext.execute_with(|| System::set_block_number(1));
		ext
	}

	#[test]
	fn foobar() {
		let balances = vec![(ALICE, CUSTOM_INITIAL_BALANCE)];
		new_test_ext_with_balances(balances).execute_with(|| {
			let precompile_addr = H160::from(
				hex::const_decode_to_array(b"00000000000000000000000000000000000B0000").unwrap(),
			);

			let mut msg: Vec<u8> = Vec::new();
			msg.push(123);
			let echo_params = IDemo::echoCall { mode: 1, message: Bytes::from(msg) };
			let call = IDemo::IDemoCalls::echo(echo_params);
			let encoded_call = call.abi_encode();

			let result = pallet_revive::Pallet::<Test>::bare_call(
				RuntimeOrigin::signed(ALICE),
				precompile_addr,
				U256::zero(),
				Weight::MAX,
				u128::MAX,
				encoded_call,
				ExecConfig::new_substrate_tx(),
			);
			assert!(result.result.is_ok());
			let data = result.result.unwrap().data;
			let bytes = Bytes::abi_decode_validate(&data).unwrap();
			let vec: Vec<u8> = bytes.0.into();
			assert_eq!(vec, vec![123]);
		});
	}
}
