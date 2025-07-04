use anyhow::Result;
use clap::Parser;
use ethers::providers::Middleware;
use std::str::FromStr;

use artemis_core::engine::Engine;
use artemis_core::types::{CollectorMap, ExecutorMap};
use collectors::time_collector::TimeCollector;
use ethers::{
    prelude::MiddlewareBuilder,
    providers::{Http, Provider},
    signers::{LocalWallet, Signer},
};
use executors::protect_executor::ProtectExecutor;
use std::sync::Arc;
use strategies::{
    aave_strategy::{AaveStrategy, Deployment},
    types::{Action, Config, Event},
};
use tracing::{info, Level};
use tracing_subscriber::{filter, prelude::*};

pub mod collectors;
pub mod executors;
pub mod strategies;

static POLL_INTERVAL_SECS: u64 = 60 * 5;

/// CLI Options.
#[derive(Parser, Debug)]
pub struct Args {
    /// Ethereum node WS endpoint.
    #[arg(long, env = "RPC", value_name = "URL", required = true)]
    pub rpc: String,

    /// Private key for sending txs.
    #[arg(long, env = "PRIVATE_KEY", value_name = "HEX", required = true)]
    pub private_key: String,

    /// Percentage of profit to pay in gas.
    #[arg(long, env = "BID_PERCENTAGE", value_name = "U64", default_value_t = 50)]
    pub bid_percentage: u64,

    #[arg(long, env = "DEPLOYMENT", required = true)]
    pub deployment: Deployment,

    #[arg(long, env = "LIQUIDATOR_ADDRESS", required = true)]
    pub liquidator_address: String,

    /// Whether to use the Aave offical liquidator interface.
    #[arg(long, env = "USE_AAVE_LIQUIDATOR", default_value_t = false)]
    pub use_aave_liquidator: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up tracing and parse args.
    let filter = filter::Targets::new()
        .with_target("artemis_core", Level::INFO)
        .with_target("aave_v3_liquidator", Level::INFO);

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let args = Args::parse();
    println!("{:?}", args);

    // Set up ethers provider.
    let rpc = Http::from_str(&args.rpc)?;
    let provider = Provider::new(rpc);

    let chain_id = provider.get_chainid().await?.as_u64();
    println!("Using chain ID: {}", chain_id);

    let wallet: LocalWallet = args
        .private_key
        .parse::<LocalWallet>()
        .unwrap()
        .with_chain_id(chain_id);
    let address = wallet.address();

    let provider = Arc::new(provider.nonce_manager(address).with_signer(wallet.clone()));

    // Set up engine.
    let mut engine: Engine<Event, Action> = Engine::default();

    // Set up time collector.
    let time_collector = Box::new(TimeCollector::new(POLL_INTERVAL_SECS));
    let time_collector = CollectorMap::new(time_collector, Event::NewTick);
    engine.add_collector(Box::new(time_collector));

    let config = Config {
        bid_percentage: args.bid_percentage,
        chain_id,
    };

    let strategy = AaveStrategy::new(
        Arc::new(provider.clone()),
        config,
        args.deployment,
        args.liquidator_address,
        args.use_aave_liquidator,
    );
    engine.add_strategy(Box::new(strategy));

    let executor = Box::new(ProtectExecutor::new(provider.clone(), provider.clone()));

    let executor = ExecutorMap::new(executor, |action| match action {
        Action::SubmitTx(tx) => Some(tx),
    });

    engine.add_executor(Box::new(executor));
    // Start engine.
    if let Ok(mut set) = engine.run().await {
        while let Some(res) = set.join_next().await {
            info!("res: {:?}", res);
        }
    }
    Ok(())
}
