//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

pub mod dev;

// std
use std::{sync::Arc, time::Duration};

// Local Runtime Types
use ink_parachain_runtime::{
	opaque::{Block, Hash},
	RuntimeApi,
};
// Cumulus Imports
use cumulus_client_cli::CollatorOptions;
use cumulus_client_collator::service::CollatorService;
use cumulus_client_consensus_aura::collators::lookahead::{self as aura, Params as AuraParams};
use cumulus_client_consensus_common::ParachainBlockImport as TParachainBlockImport;
use cumulus_client_parachain_inherent::MockValidationDataInherentDataProvider;
use cumulus_client_service::{
	build_network, build_relay_chain_interface, prepare_node_config, start_relay_chain_tasks,
	BuildNetworkParams, CollatorSybilResistance, DARecoveryProfile, ParachainHostFunctions,
	StartRelayChainTasksParams,
};
use cumulus_primitives_core::{
	relay_chain::{CollatorPair, ValidationCode},
	ParaId,
};
use cumulus_relay_chain_interface::{OverseerHandle, RelayChainInterface};
use polkadot_primitives::{HeadData, UpgradeGoAhead};

// Substrate Imports
use codec::Encode;
use cumulus_primitives_core::CollectCollationInfo;
use frame_benchmarking_cli::SUBSTRATE_REFERENCE_HARDWARE;
use prometheus_endpoint::Registry;
use sc_client_api::Backend;
use sc_consensus::{ImportQueue, LongestChain};
use sc_consensus_manual_seal::consensus::aura::AuraConsensusDataProvider;
use sc_executor::{HeapAllocStrategy, WasmExecutor, DEFAULT_HEAP_ALLOC_STRATEGY};
use sc_network::{NetworkBackend, NetworkBlock};
use sc_service::{Configuration, PartialComponents, TFullBackend, TFullClient, TaskManager};
use sc_telemetry::{Telemetry, TelemetryHandle, TelemetryWorker, TelemetryWorkerHandle};
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use sp_api::ProvideRuntimeApi;
use sp_keystore::KeystorePtr;
use sp_runtime::traits::UniqueSaturatedInto;

type ParachainExecutor = WasmExecutor<ParachainHostFunctions>;

type ParachainClient = TFullClient<Block, RuntimeApi, ParachainExecutor>;

type ParachainBackend = TFullBackend<Block>;

type ParachainBlockImport = TParachainBlockImport<Block, Arc<ParachainClient>, ParachainBackend>;

/// Assembly of PartialComponents (enough to run chain ops subcommands)
pub type Service = PartialComponents<
	ParachainClient,
	ParachainBackend,
	(),
	sc_consensus::DefaultImportQueue<Block>,
	sc_transaction_pool::TransactionPoolHandle<Block, ParachainClient>,
	(ParachainBlockImport, Option<Telemetry>, Option<TelemetryWorkerHandle>),
>;

/// Starts a `ServiceBuilder` for a full service.
///
/// Use this macro if you don't actually need the full service, but just the builder in order to
/// be able to perform chain operations.
pub fn new_partial(config: &Configuration) -> Result<Service, sc_service::Error> {
	let telemetry = config
		.telemetry_endpoints
		.clone()
		.filter(|x| !x.is_empty())
		.map(|endpoints| -> Result<_, sc_telemetry::Error> {
			let worker = TelemetryWorker::new(16)?;
			let telemetry = worker.handle().new_telemetry(endpoints);
			Ok((worker, telemetry))
		})
		.transpose()?;

	let heap_pages = config
		.executor
		.default_heap_pages
		.map_or(DEFAULT_HEAP_ALLOC_STRATEGY, |h| HeapAllocStrategy::Static { extra_pages: h as _ });

	let executor = ParachainExecutor::builder()
		.with_execution_method(config.executor.wasm_method)
		.with_onchain_heap_alloc_strategy(heap_pages)
		.with_offchain_heap_alloc_strategy(heap_pages)
		.with_max_runtime_instances(config.executor.max_runtime_instances)
		.with_runtime_cache_size(config.executor.runtime_cache_size)
		.build();

	let (client, backend, keystore_container, task_manager) =
		sc_service::new_full_parts_record_import::<Block, RuntimeApi, _>(
			config,
			telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
			executor,
			true,
		)?;
	let client = Arc::new(client);

	let telemetry_worker_handle = telemetry.as_ref().map(|(worker, _)| worker.handle());

	let telemetry = telemetry.map(|(worker, telemetry)| {
		task_manager.spawn_handle().spawn("telemetry", None, worker.run());
		telemetry
	});

	let transaction_pool = Arc::from(
		sc_transaction_pool::Builder::new(
			task_manager.spawn_essential_handle(),
			client.clone(),
			config.role.is_authority().into(),
		)
		.with_options(config.transaction_pool.clone())
		.with_prometheus(config.prometheus_registry())
		.build(),
	);

	let block_import = ParachainBlockImport::new(client.clone(), backend.clone());

	let import_queue = build_import_queue(
		client.clone(),
		block_import.clone(),
		config,
		telemetry.as_ref().map(|telemetry| telemetry.handle()),
		&task_manager,
	);

	Ok(PartialComponents {
		backend,
		client,
		import_queue,
		keystore_container,
		task_manager,
		transaction_pool,
		select_chain: (),
		other: (block_import, telemetry, telemetry_worker_handle),
	})
}

/// Build the import queue for the parachain runtime.
fn build_import_queue(
	client: Arc<ParachainClient>,
	block_import: ParachainBlockImport,
	config: &Configuration,
	telemetry: Option<TelemetryHandle>,
	task_manager: &TaskManager,
) -> sc_consensus::DefaultImportQueue<Block> {
	cumulus_client_consensus_aura::equivocation_import_queue::fully_verifying_import_queue::<
		sp_consensus_aura::sr25519::AuthorityPair,
		_,
		_,
		_,
		_,
	>(
		client,
		block_import,
		move |_, _| async move {
			let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
			Ok(timestamp)
		},
		&task_manager.spawn_essential_handle(),
		config.prometheus_registry(),
		telemetry,
	)
}

#[allow(clippy::too_many_arguments)]
fn start_consensus(
	client: Arc<ParachainClient>,
	backend: Arc<ParachainBackend>,
	block_import: ParachainBlockImport,
	prometheus_registry: Option<&Registry>,
	telemetry: Option<TelemetryHandle>,
	task_manager: &TaskManager,
	relay_chain_interface: Arc<dyn RelayChainInterface>,
	transaction_pool: Arc<sc_transaction_pool::TransactionPoolHandle<Block, ParachainClient>>,
	keystore: KeystorePtr,
	relay_chain_slot_duration: Duration,
	para_id: ParaId,
	collator_key: CollatorPair,
	overseer_handle: OverseerHandle,
	announce_block: Arc<dyn Fn(Hash, Option<Vec<u8>>) + Send + Sync>,
) -> Result<(), sc_service::Error> {
	let proposer = sc_basic_authorship::ProposerFactory::with_proof_recording(
		task_manager.spawn_handle(),
		client.clone(),
		transaction_pool,
		prometheus_registry,
		telemetry.clone(),
	);

	let collator_service = CollatorService::new(
		client.clone(),
		Arc::new(task_manager.spawn_handle()),
		announce_block,
		client.clone(),
	);

	let params = AuraParams {
		create_inherent_data_providers: move |_, ()| async move { Ok(()) },
		block_import,
		para_client: client.clone(),
		para_backend: backend,
		relay_client: relay_chain_interface,
		code_hash_provider: move |block_hash| {
			client.code_at(block_hash).ok().map(|c| ValidationCode::from(c).hash())
		},
		keystore,
		collator_key,
		para_id,
		overseer_handle,
		relay_chain_slot_duration,
		proposer,
		collator_service,
		authoring_duration: Duration::from_millis(2000),
		reinitialize: false,
		max_pov_percentage: None,
	};

	let fut = aura::run::<Block, sp_consensus_aura::sr25519::AuthorityPair, _, _, _, _, _, _, _, _>(
		params,
	);
	task_manager.spawn_essential_handle().spawn("aura", None, fut);

	Ok(())
}

/// Start a node with the given parachain `Configuration` and relay chain `Configuration`.
#[sc_tracing::logging::prefix_logs_with("Parachain")]
pub async fn start_parachain_node(
	parachain_config: Configuration,
	polkadot_config: Configuration,
	collator_options: CollatorOptions,
	para_id: ParaId,
	hwbench: Option<sc_sysinfo::HwBench>,
) -> sc_service::error::Result<(TaskManager, Arc<ParachainClient>)> {
	let parachain_config = prepare_node_config(parachain_config);

	let params = new_partial(&parachain_config)?;
	let (block_import, mut telemetry, telemetry_worker_handle) = params.other;
	let prometheus_registry = parachain_config.prometheus_registry().cloned();
	let net_config = sc_network::config::FullNetworkConfiguration::<
		_,
		_,
		sc_network::NetworkWorker<Block, Hash>,
	>::new(&parachain_config.network, prometheus_registry.clone());

	let client = params.client.clone();
	let backend = params.backend.clone();
	let mut task_manager = params.task_manager;

	let (relay_chain_interface, collator_key, _, _) = build_relay_chain_interface(
		polkadot_config,
		&parachain_config,
		telemetry_worker_handle,
		&mut task_manager,
		collator_options.clone(),
		hwbench.clone(),
	)
	.await
	.map_err(|e| sc_service::Error::Application(Box::new(e) as Box<_>))?;

	let validator = parachain_config.role.is_authority();
	let transaction_pool = params.transaction_pool.clone();
	let import_queue_service = params.import_queue.service();

	// NOTE: because we use Aura here explicitly, we can use `CollatorSybilResistance::Resistant`
	// when starting the network.
	let (network, system_rpc_tx, tx_handler_controller, sync_service) =
		build_network(BuildNetworkParams {
			parachain_config: &parachain_config,
			net_config,
			client: client.clone(),
			transaction_pool: transaction_pool.clone(),
			para_id,
			spawn_handle: task_manager.spawn_handle(),
			relay_chain_interface: relay_chain_interface.clone(),
			import_queue: params.import_queue,
			sybil_resistance_level: CollatorSybilResistance::Resistant, // because of Aura
			metrics: sc_network::NetworkWorker::<Block, Hash>::register_notification_metrics(
				parachain_config.prometheus_config.as_ref().map(|config| &config.registry),
			),
		})
		.await?;

	if parachain_config.offchain_worker.enabled {
		use futures::FutureExt;

		let offchain_workers =
			sc_offchain::OffchainWorkers::new(sc_offchain::OffchainWorkerOptions {
				runtime_api_provider: client.clone(),
				keystore: Some(params.keystore_container.keystore()),
				offchain_db: backend.offchain_storage(),
				transaction_pool: Some(OffchainTransactionPoolFactory::new(
					transaction_pool.clone(),
				)),
				network_provider: Arc::new(network.clone()),
				is_validator: parachain_config.role.is_authority(),
				enable_http_requests: false,
				custom_extensions: move |_| vec![],
			})?;
		task_manager.spawn_handle().spawn(
			"offchain-workers-runner",
			"offchain-work",
			offchain_workers.run(client.clone(), task_manager.spawn_handle()).boxed(),
		);
	}

	let rpc_builder = {
		let client = client.clone();
		let transaction_pool = transaction_pool.clone();

		Box::new(move |_| {
			let deps =
				crate::rpc::FullDeps { client: client.clone(), pool: transaction_pool.clone() };

			crate::rpc::create_full(deps).map_err(Into::into)
		})
	};

	sc_service::spawn_tasks(sc_service::SpawnTasksParams {
		rpc_builder,
		client: client.clone(),
		transaction_pool: transaction_pool.clone(),
		task_manager: &mut task_manager,
		config: parachain_config,
		keystore: params.keystore_container.keystore(),
		backend: backend.clone(),
		network,
		sync_service: sync_service.clone(),
		system_rpc_tx,
		tx_handler_controller,
		telemetry: telemetry.as_mut(),
		tracing_execute_block: None,
	})?;

	if let Some(hwbench) = hwbench {
		sc_sysinfo::print_hwbench(&hwbench);
		// Here you can check whether the hardware meets your chains' requirements. Putting a link
		// in there and swapping out the requirements for your own are probably a good idea. The
		// requirements for a para-chain are dictated by its relay-chain.
		match SUBSTRATE_REFERENCE_HARDWARE.check_hardware(&hwbench, false) {
			Err(err) if validator => {
				log::warn!(
				"⚠️  The hardware does not meet the minimal requirements {} for role 'Authority'.",
				err
			);
			},
			_ => {},
		}

		if let Some(ref mut telemetry) = telemetry {
			let telemetry_handle = telemetry.handle();
			task_manager.spawn_handle().spawn(
				"telemetry_hwbench",
				None,
				sc_sysinfo::initialize_hwbench_telemetry(telemetry_handle, hwbench),
			);
		}
	}

	let announce_block = {
		let sync_service = sync_service.clone();
		Arc::new(move |hash, data| sync_service.announce_block(hash, data))
	};

	let relay_chain_slot_duration = Duration::from_secs(6);

	let overseer_handle = relay_chain_interface
		.overseer_handle()
		.map_err(|e| sc_service::Error::Application(Box::new(e)))?;

	start_relay_chain_tasks(StartRelayChainTasksParams {
		client: client.clone(),
		announce_block: announce_block.clone(),
		para_id,
		relay_chain_interface: relay_chain_interface.clone(),
		task_manager: &mut task_manager,
		da_recovery_profile: if validator {
			DARecoveryProfile::Collator
		} else {
			DARecoveryProfile::FullNode
		},
		import_queue: import_queue_service,
		relay_chain_slot_duration,
		recovery_handle: Box::new(overseer_handle.clone()),
		sync_service: sync_service.clone(),
		prometheus_registry: prometheus_registry.as_ref(),
	})?;

	if validator {
		start_consensus(
			client.clone(),
			backend,
			block_import,
			prometheus_registry.as_ref(),
			telemetry.as_ref().map(|t| t.handle()),
			&task_manager,
			relay_chain_interface,
			transaction_pool,
			params.keystore_container.keystore(),
			relay_chain_slot_duration,
			para_id,
			collator_key.expect("Command line arguments do not allow this. qed"),
			overseer_handle,
			announce_block,
		)?;
	}

	Ok((task_manager, client))
}

/// Creates the inherent data providers for dev mode (instant/manual seal).
///
/// This function sets up the timestamp and parachain validation data providers
/// required for dev seal block production in a parachain environment without a relay chain.
fn create_dev_inherent_data_providers(
	client: Arc<ParachainClient>,
	para_id: ParaId,
	slot_duration: sp_consensus_aura::SlotDuration,
) -> impl Fn(
	Hash,
	(),
) -> std::future::Ready<
	Result<
		(sp_timestamp::InherentDataProvider, MockValidationDataInherentDataProvider<()>),
		Box<dyn std::error::Error + Send + Sync>,
	>,
> + Send
       + Sync {
	move |parent_hash: Hash, ()| {
		let current_para_head = client
			.header(parent_hash)
			.expect("Header lookup should succeed")
			.expect("Header passed in as parent should be present in backend.");

		// Check if there's pending validation code that needs upgrade signal
		let should_send_go_ahead = client
			.runtime_api()
			.collect_collation_info(parent_hash, &current_para_head)
			.map(|info| info.new_validation_code.is_some())
			.unwrap_or_default();

		let current_para_block_head = Some(HeadData(current_para_head.encode()));
		let current_block_number =
			UniqueSaturatedInto::<u32>::unique_saturated_into(current_para_head.number) + 1;

		// Create mock parachain validation data
		let mocked_parachain = MockValidationDataInherentDataProvider::<()> {
			current_para_block: current_block_number,
			para_id,
			current_para_block_head,
			relay_blocks_per_para_block: 1,
			para_blocks_per_relay_epoch: 10,
			upgrade_go_ahead: should_send_go_ahead.then(|| {
				log::info!("Detected pending validation code, sending go-ahead signal.");
				UpgradeGoAhead::GoAhead
			}),
			..Default::default()
		};

		// Create timestamp aligned to Aura slot timing
		let timestamp = sp_timestamp::InherentDataProvider::new(
			(slot_duration.as_millis() * current_block_number as u64).into(),
		);

		std::future::ready(Ok((timestamp, mocked_parachain)))
	}
}

/// Start a parachain node in dev mode with instant seal.
///
/// This allows the parachain to run standalone without connecting to a relay chain,
/// useful for development and testing.
pub async fn start_dev_parachain_node(
	mut config: Configuration,
	finalize_delay_sec: u64,
) -> sc_service::error::Result<TaskManager> {
	// Since this is a dev node, prevent it from connecting to peers
	config.network.default_peers_set.in_peers = 0;
	config.network.default_peers_set.out_peers = 0;

	// Build client, backend, and other components manually
	// We can't use new_partial() because it creates an Aura import queue
	let telemetry = config
		.telemetry_endpoints
		.clone()
		.filter(|x| !x.is_empty())
		.map(|endpoints| -> Result<_, sc_telemetry::Error> {
			let worker = TelemetryWorker::new(16)?;
			let telemetry = worker.handle().new_telemetry(endpoints);
			Ok((worker, telemetry))
		})
		.transpose()?;

	let heap_pages = config
		.executor
		.default_heap_pages
		.map_or(DEFAULT_HEAP_ALLOC_STRATEGY, |h| HeapAllocStrategy::Static { extra_pages: h as _ });

	let executor = ParachainExecutor::builder()
		.with_execution_method(config.executor.wasm_method)
		.with_onchain_heap_alloc_strategy(heap_pages)
		.with_offchain_heap_alloc_strategy(heap_pages)
		.with_max_runtime_instances(config.executor.max_runtime_instances)
		.with_runtime_cache_size(config.executor.runtime_cache_size)
		.build();

	let (client, backend, keystore_container, mut task_manager) =
		sc_service::new_full_parts_record_import::<Block, RuntimeApi, _>(
			&config,
			telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
			executor,
			true,
		)?;
	let client = Arc::new(client);

	let mut telemetry = telemetry.map(|(worker, telemetry)| {
		task_manager.spawn_handle().spawn("telemetry", None, worker.run());
		telemetry
	});

	let transaction_pool = Arc::from(
		sc_transaction_pool::Builder::new(
			task_manager.spawn_essential_handle(),
			client.clone(),
			config.role.is_authority().into(),
		)
		.with_options(config.transaction_pool.clone())
		.with_prometheus(config.prometheus_registry())
		.build(),
	);

	// Create manual seal import queue (accepts all blocks without relay chain validation)
	let import_queue = sc_consensus_manual_seal::import_queue(
		Box::new(client.clone()),
		&task_manager.spawn_essential_handle(),
		config.prometheus_registry(),
	);

	let net_config = sc_network::config::FullNetworkConfiguration::<
		_,
		_,
		sc_network::NetworkWorker<Block, Hash>,
	>::new(
		&config.network,
		config.prometheus_config.as_ref().map(|cfg| cfg.registry.clone()),
	);

	let (network, system_rpc_tx, tx_handler_controller, sync_service) =
		sc_service::build_network(sc_service::BuildNetworkParams {
			config: &config,
			client: client.clone(),
			transaction_pool: transaction_pool.clone(),
			spawn_handle: task_manager.spawn_handle(),
			import_queue,
			net_config,
			block_announce_validator_builder: None,
			warp_sync_config: None,
			block_relay: None,
			metrics: sc_network::NetworkWorker::<Block, Hash>::register_notification_metrics(
				config.prometheus_config.as_ref().map(|config| &config.registry),
			),
		})?;

	if config.offchain_worker.enabled {
		use futures::FutureExt;

		let offchain_workers =
			sc_offchain::OffchainWorkers::new(sc_offchain::OffchainWorkerOptions {
				runtime_api_provider: client.clone(),
				keystore: Some(keystore_container.keystore()),
				offchain_db: backend.offchain_storage(),
				transaction_pool: Some(OffchainTransactionPoolFactory::new(
					transaction_pool.clone(),
				)),
				network_provider: Arc::new(network.clone()),
				is_validator: config.role.is_authority(),
				enable_http_requests: true,
				custom_extensions: move |_| vec![],
			})?;
		task_manager.spawn_handle().spawn(
			"offchain-workers-runner",
			"offchain-work",
			offchain_workers.run(client.clone(), task_manager.spawn_handle()).boxed(),
		);
	}

	let proposer = sc_basic_authorship::ProposerFactory::new(
		task_manager.spawn_handle(),
		client.clone(),
		transaction_pool.clone(),
		None,
		None,
	);

	// Get slot duration from runtime
	let slot_duration = sc_consensus_aura::slot_duration(&*client)
		.expect("Slot duration is always present in Aura runtime; qed.");

	// The aura digest provider will provide digests that match the provided timestamp data.
	// Without this, the AURA parachain runtime complains about slot mismatches.
	let aura_digest_provider = AuraConsensusDataProvider::new_with_slot_duration(slot_duration);

	// Extract para_id from chain spec
	let para_id = crate::chain_spec::Extensions::try_get(&*config.chain_spec)
		.map(|e| e.para_id)
		.ok_or("Could not find parachain ID in chain-spec.")?;
	let para_id = ParaId::from(para_id);

	// Create inherent data providers with mocked relay chain data
	let create_inherent_data_providers =
		create_dev_inherent_data_providers(client.clone(), para_id, slot_duration);

	// Spawn instant seal consensus
	let params = sc_consensus_manual_seal::InstantSealParams {
		block_import: client.clone(),
		env: proposer,
		client: client.clone(),
		pool: transaction_pool.clone(),
		select_chain: LongestChain::new(backend.clone()),
		consensus_data_provider: Some(Box::new(aura_digest_provider)),
		create_inherent_data_providers,
	};

	let authorship_future = sc_consensus_manual_seal::run_instant_seal(params);
	task_manager
		.spawn_essential_handle()
		.spawn_blocking("instant-seal", None, authorship_future);

	// Optional delayed finalization
	if finalize_delay_sec > 0 {
		let delayed_finalize_params = sc_consensus_manual_seal::DelayedFinalizeParams {
			client: client.clone(),
			spawn_handle: task_manager.spawn_handle(),
			delay_sec: finalize_delay_sec,
		};
		task_manager.spawn_essential_handle().spawn_blocking(
			"delayed_finalize",
			None,
			sc_consensus_manual_seal::run_delayed_finalize(delayed_finalize_params),
		);
	}

	let rpc_builder = {
		let client = client.clone();
		let transaction_pool = transaction_pool.clone();

		Box::new(move |_| {
			let deps =
				crate::rpc::FullDeps { client: client.clone(), pool: transaction_pool.clone() };

			crate::rpc::create_full(deps).map_err(Into::into)
		})
	};

	let _rpc_handlers = sc_service::spawn_tasks(sc_service::SpawnTasksParams {
		network,
		client,
		keystore: keystore_container.keystore(),
		task_manager: &mut task_manager,
		transaction_pool,
		rpc_builder,
		backend,
		system_rpc_tx,
		tx_handler_controller,
		sync_service,
		config,
		telemetry: telemetry.as_mut(),
		tracing_execute_block: None,
	})?;

	Ok(task_manager)
}
