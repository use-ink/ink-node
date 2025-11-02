#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use core::marker::PhantomData;
use pallet_revive::precompiles::{
	alloy::{self, sol_types::SolCall},
	AddressMatcher, Error, Ext, Precompile,
};

alloy::sol!("src/IFoo.sol");
use IFoo::IFooCalls;

const LOG_TARGET: &str = "johan::precompile";

pub trait FooPrecompileConfig {
	/// The Address matcher used by the precompile.
	const MATCHER: AddressMatcher;
}

/// A precompile configuration that uses a prefix [`AddressMatcher`].
pub struct InlineIdConfig<const PREFIX: u16>;

impl<const P: u16> FooPrecompileConfig for InlineIdConfig<P> {
	const MATCHER: AddressMatcher = AddressMatcher::Fixed(core::num::NonZero::new(P).unwrap());
}

/// A Foo precompile.
pub struct Foo<Runtime, PrecompileConfig, Instance = ()> {
	_phantom: PhantomData<(Runtime, PrecompileConfig, Instance)>,
}

impl<Runtime, PrecompileConfig, Instance: 'static> Precompile
	for Foo<Runtime, PrecompileConfig, Instance>
where
	PrecompileConfig: FooPrecompileConfig,
	Runtime: pallet_revive::Config,
	//Runtime: crate::Config<Instance> + pallet_revive::Config,
{
	type T = Runtime;
	type Interface = IFoo::IFooCalls;
	const MATCHER: AddressMatcher = PrecompileConfig::MATCHER;
	const HAS_CONTRACT_INFO: bool = false;

	fn call(
		_address: &[u8; 20],
		input: &Self::Interface,
		_env: &mut impl Ext<T = Self::T>,
	) -> Result<Vec<u8>, Error> {
		log::info!(target: LOG_TARGET, "Fortytwo: ☃️☃️☃️☃️☃️");
		match input {
			IFooCalls::fortytwo(_) => Self::fortytwo(),
		}
	}
}

impl<Runtime, PrecompileConfig, Instance: 'static> Foo<Runtime, PrecompileConfig, Instance>
where
	PrecompileConfig: FooPrecompileConfig,
	Runtime: pallet_revive::Config,
{
	fn fortytwo() -> Result<Vec<u8>, Error> {
		log::info!(target: LOG_TARGET, "Fortytwo: 👾👾👾👾👾👾👾");
		return Ok(IFoo::fortytwoCall::abi_encode_returns(&42u128));
	}
}
