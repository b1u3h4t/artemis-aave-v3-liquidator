use super::types::Config;
use crate::collectors::time_collector::NewTick;
use anyhow::{anyhow, Result};
use artemis_core::executors::mempool_executor::{GasBidInfo, SubmitTxToMempool};
use artemis_core::types::Strategy;
use async_trait::async_trait;
use bindings_aave::{
    i_aave_oracle::IAaveOracle,
    i_pool_data_provider::IPoolDataProvider,
    ierc20::IERC20,
    l2_encoder::L2Encoder,
    pool::{BorrowFilter, Pool, SupplyFilter},
};
use bindings_liquidator::liquidator::Liquidator;
use clap::{Parser, ValueEnum};
use ethers::{
    contract::builders::ContractCall,
    providers::Middleware,
    types::{transaction::eip2718::TypedTransaction, Address, ValueOrArray, H160, I256, U256, U64},
};
use ethers_contract::Multicall;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::iter::zip;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{error, info};

use super::types::{Action, Event};

#[derive(Debug)]
struct DeploymentConfig {
    pool_address: Address,
    pool_data_provider: Address,
    oracle_address: Address,
    l2_encoder: Address,
    creation_block: u64,
    weth_address: Address,
}

#[derive(Debug, Clone, Parser, ValueEnum)]
pub enum Deployment {
    AAVE,
    SEASHELL,
    AaveV3Sonic,
    AaveV3Celo,
    AaveV3Ethereum,
    AaveV3Optimism,
    AaveV3Bnb,
    AaveV3Arbitrum,
    AaveV3Avax,
    AaveV3Polygon,
}

pub const WETH_ADDRESS: &str = "0x4200000000000000000000000000000000000006";

pub const LIQUIDATION_CLOSE_FACTOR_THRESHOLD: &str = "950000000000000000";
pub const MAX_LIQUIDATION_CLOSE_FACTOR: u64 = 10000;
pub const DEFAULT_LIQUIDATION_CLOSE_FACTOR: u64 = 5000;

// admin stuff
pub const LOG_BLOCK_RANGE: u64 = 1024;
pub const MULTICALL_CHUNK_SIZE: usize = 500;
pub const STATE_CACHE_FILE: &str = "borrowers.json";
pub const PRICE_ONE: u64 = 100000000;

fn get_deployment_config(deployment: Deployment) -> DeploymentConfig {
    match deployment {
        Deployment::AAVE => DeploymentConfig {
            pool_address: Address::from_str("0xA238Dd80C259a72e81d7e4664a9801593F98d1c5").unwrap(),
            pool_data_provider: Address::from_str("0x2d8A3C5677189723C4cB8873CfC9C8976FDF38Ac")
                .unwrap(),
            oracle_address: Address::from_str("0x2Cc0Fc26eD4563A5ce5e8bdcfe1A2878676Ae156")
                .unwrap(),
            l2_encoder: Address::from_str("0x39e97c588B2907Fb67F44fea256Ae3BA064207C5").unwrap(),
            creation_block: 2963358,
            weth_address: Address::from_str(WETH_ADDRESS).unwrap(),
        },
        Deployment::SEASHELL => DeploymentConfig {
            pool_address: Address::from_str("0x8F44Fd754285aa6A2b8B9B97739B79746e0475a7").unwrap(),
            pool_data_provider: Address::from_str("0x2A0979257105834789bC6b9E1B00446DFbA8dFBa")
                .unwrap(),
            oracle_address: Address::from_str("0xFDd4e83890BCcd1fbF9b10d71a5cc0a738753b01")
                .unwrap(),
            l2_encoder: Address::from_str("0xceceF475167f7BFD8995c0cbB577644b623cD7Cf").unwrap(),
            creation_block: 3318602,
            weth_address: Address::from_str(WETH_ADDRESS).unwrap(),
        },
        Deployment::AaveV3Sonic => DeploymentConfig {
            pool_address: Address::from_str("0x5362dBb1e601abF3a4c14c22ffEdA64042E5eAA3").unwrap(),
            pool_data_provider: Address::from_str("0x306c124fFba5f2Bc0BcAf40D249cf19D492440b9")
                .unwrap(),
            oracle_address: Address::from_str("0xD63f7658C66B2934Bd234D79D06aEF5290734B30")
                .unwrap(),
            l2_encoder: Address::zero(),
            creation_block: 7986580,
            weth_address: Address::from_str("0x039e2fB66102314Ce7b64Ce5Ce3E5183bc94aD38").unwrap(),
        },
        Deployment::AaveV3Celo => DeploymentConfig {
            pool_address: Address::from_str("0x3E59A31363E2ad014dcbc521c4a0d5757d9f3402").unwrap(),
            pool_data_provider: Address::from_str("0x33b7d355613110b4E842f5f7057Ccd36fb4cee28")
                .unwrap(),
            oracle_address: Address::from_str("0x1e693D088ceFD1E95ba4c4a5F7EeA41a1Ec37e8b")
                .unwrap(),
            l2_encoder: Address::zero(),
            creation_block: 30390066,
            weth_address: Address::from_str("0x471EcE3750Da237f93B8E339c536989b8978a438").unwrap(),
        },
        Deployment::AaveV3Ethereum => DeploymentConfig {
            pool_address: Address::from_str("0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2").unwrap(),
            pool_data_provider: Address::from_str("0x497a1994c46d4f6C864904A9f1fac6328Cb7C8a6")
                .unwrap(),
            oracle_address: Address::from_str("0x54586bE62E3c3580375aE3723C145253060Ca0C2")
                .unwrap(),
            l2_encoder: Address::zero(),
            creation_block: 16291126,
            weth_address: Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap(),
        },
        Deployment::AaveV3Optimism => DeploymentConfig {
            pool_address: Address::from_str("0x794a61358D6845594F94dc1DB02A252b5b4814aD").unwrap(),
            pool_data_provider: Address::from_str("0x14496b405D62c24F91f04Cda1c69Dc526D56fDE5")
                .unwrap(),
            oracle_address: Address::from_str("0xD81eb3728a631871a7eBBaD631b5f424909f0c77")
                .unwrap(),
            l2_encoder: Address::from_str("0x9abADECD08572e0eA5aF4d47A9C7984a5AA503dC").unwrap(),
            creation_block: 4365693,
            weth_address: Address::from_str(WETH_ADDRESS).unwrap(),
        },
        Deployment::AaveV3Bnb => DeploymentConfig {
            pool_address: Address::from_str("0x6807dc923806fE8Fd134338EABCA509979a7e0cB").unwrap(),
            pool_data_provider: Address::from_str("0x1e26247502e90b4fab9D0d17e4775e90085D2A35")
                .unwrap(),
            oracle_address: Address::from_str("0x39bc1bfDa2130d6Bb6DBEfd366939b4c7aa7C697")
                .unwrap(),
            l2_encoder: Address::zero(),
            creation_block: 33571625,
            weth_address: Address::from_str("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c").unwrap(),
        },
        Deployment::AaveV3Arbitrum => DeploymentConfig {
            pool_address: Address::from_str("0x794a61358D6845594F94dc1DB02A252b5b4814aD").unwrap(),
            pool_data_provider: Address::from_str("0x14496b405D62c24F91f04Cda1c69Dc526D56fDE5")
                .unwrap(),
            oracle_address: Address::from_str("0xb56c2F0B653B2e0b10C9b928C8580Ac5Df02C7C7")
                .unwrap(),
            l2_encoder: Address::from_str("0x9abADECD08572e0eA5aF4d47A9C7984a5AA503dC").unwrap(),
            creation_block: 7742429,
            weth_address: Address::from_str("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1").unwrap(),
        },
        Deployment::AaveV3Avax => DeploymentConfig {
            pool_address: Address::from_str("0x794a61358D6845594F94dc1DB02A252b5b4814aD").unwrap(),
            pool_data_provider: Address::from_str("0x14496b405D62c24F91f04Cda1c69Dc526D56fDE5")
                .unwrap(),
            oracle_address: Address::from_str("0xEBd36016B3eD09D4693Ed4251c67Bd858c3c7C9C")
                .unwrap(),
            l2_encoder: Address::zero(),
            creation_block: 11970506,
            weth_address: Address::from_str("0xB31f66AA3C1e785363F0875A1B74E27b85FD66c7").unwrap(),
        },
        Deployment::AaveV3Polygon => DeploymentConfig {
            pool_address: Address::from_str("0x794a61358D6845594F94dc1DB02A252b5b4814aD").unwrap(),
            pool_data_provider: Address::from_str("0x14496b405D62c24F91f04Cda1c69Dc526D56fDE5")
                .unwrap(),
            oracle_address: Address::from_str("0xb023e699F5a33916Ea823A16485e259257cA8Bd1")
                .unwrap(),
            l2_encoder: Address::zero(),
            creation_block: 25826028,
            weth_address: Address::from_str("0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270").unwrap(),
        },
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StateCache {
    last_block_number: u64,
    borrowers: HashMap<Address, Borrower>,
}

struct PoolState {
    prices: HashMap<Address, U256>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Borrower {
    address: Address,
    collateral: HashSet<Address>,
    debt: HashSet<Address>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenConfig {
    address: Address,
    a_address: Address,
    decimals: u64,
    ltv: u64,
    liquidation_threshold: u64,
    liquidation_bonus: u64,
    reserve_factor: u64,
    protocol_fee: u64,
    symbol: String,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct AaveStrategy<M> {
    /// Ethers client.
    client: Arc<M>,
    /// Amount of profits to bid in gas
    bid_percentage: u64,
    last_block_number: u64,
    borrowers: HashMap<Address, Borrower>,
    tokens: HashMap<Address, TokenConfig>,
    chain_id: u64,
    config: DeploymentConfig,
    liquidator: Address,
    use_aave_liquidator: bool,
}

impl<M: Middleware + 'static> AaveStrategy<M> {
    pub fn new(
        client: Arc<M>,
        config: Config,
        deployment: Deployment,
        liquidator_address: String,
        use_aave_liquidator: bool,
    ) -> Self {
        Self {
            client,
            bid_percentage: config.bid_percentage,
            last_block_number: 0,
            borrowers: HashMap::new(),
            tokens: HashMap::new(),
            chain_id: config.chain_id,
            config: get_deployment_config(deployment),
            liquidator: Address::from_str(&liquidator_address).expect("invalid liquidator address"),
            use_aave_liquidator,
        }
    }
}

#[derive(Debug)]
struct LiquidationOpportunity {
    borrower: Address,
    collateral: Address,
    debt: Address,
    debt_to_cover: U256,
    profit_eth: I256,
    collateral_symbol: String,
    debt_symbol: String,
    profit_factor: I256,
}

#[async_trait]
impl<M: Middleware + 'static> Strategy<Event, Action> for AaveStrategy<M> {
    // In order to sync this strategy, we need to get the current bid for all Sudo pools.
    async fn sync_state(&mut self) -> Result<()> {
        info!("syncing state");

        self.update_token_configs().await?;
        self.approve_tokens().await?;
        self.load_cache()?;
        self.update_state().await?;

        info!("done syncing state");
        Ok(())
    }

    // Process incoming events, seeing if we can arb new orders, and updating the internal state on new blocks.
    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        match event {
            // Event::NewBlock(block) => self.process_new_block_event(block).await,
            Event::NewTick(block) => self.process_new_tick_event(block).await,
        }
    }
}

impl<M: Middleware + 'static> AaveStrategy<M> {
    /// Process new block events, updating the internal state.
    // async fn process_new_block_event(&mut self, event: NewBlock) -> Option<Action> {
    //     info!("received new block: {:?}", event);
    //     self.last_block_number = event.number.as_u64();
    //     None
    // }

    /// Process new block events, updating the internal state.
    async fn process_new_tick_event(&mut self, event: NewTick) -> Vec<Action> {
        info!("received new tick: {:?}", event);
        if let Err(e) = self.update_state().await {
            error!("Update State error: {}", e);
            return vec![];
        }

        info!("Total borrower count: {}", self.borrowers.len());
        let op = match self
            .get_best_liquidation_op()
            .await
            .map_err(|e| error!("Error finding liq ops: {}", e))
            .ok()
            .flatten()
        {
            Some(op) => op,
            None => {
                info!("No profitable ops, passing");
                return vec![];
            }
        };

        info!("Best op: {:?}", op);

        if op.profit_eth < I256::from(0) {
            info!("No profitable ops, passing");
            return vec![];
        }

        return vec![Action::SubmitTx(SubmitTxToMempool {
            tx: match self
                .build_liquidation(&op)
                .await
                .map_err(|e| error!("Error building liquidation: {}", e))
                .ok()
            {
                Some(tx) => tx,
                None => return vec![],
            },
            gas_bid_info: match U256::from_dec_str(&op.profit_eth.to_string()) {
                Ok(total_profit) => Some(GasBidInfo {
                    bid_percentage: self.bid_percentage,
                    total_profit,
                }),
                Err(e) => {
                    error!("Failed to bid: {}", e);
                    return vec![];
                }
            },
        })];
    }

    // for all known borrowers, return a sorted set of those with health factor < 1
    async fn get_underwater_borrowers(&mut self) -> Result<Vec<(Address, U256)>> {
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());

        let mut underwater_borrowers = Vec::new();

        // call pool.getUserAccountData(user) for each borrower
        let mut multicall = Multicall::new(
            self.client.clone(),
            Some(H160::from_str(
                "0xcA11bde05977b3631167028862bE2a173976CA11",
            )?),
        )
        .await?;
        let borrowers: Vec<&Borrower> = self
            .borrowers
            .values()
            .filter(|b| b.debt.len() > 0)
            .collect();
        let n = borrowers.len();
        let mut i = 0;
        info!("Found {} borrowers with debt", n);

        for chunk in borrowers.chunks(MULTICALL_CHUNK_SIZE) {
            multicall.clear_calls();

            for borrower in chunk {
                multicall.add_call(pool.get_user_account_data(borrower.address), false);
            }

            let result: Vec<(U256, U256, U256, U256, U256, U256)> = multicall.call_array().await?;
            for (borrower, (_, _, _, _, _, health_factor)) in zip(chunk, result) {
                if health_factor.lt(&U256::from_dec_str("1000000000000000000").unwrap()) {
                    info!(
                        "Found underwater borrower {:?} -  healthFactor: {}",
                        borrower, health_factor
                    );
                    underwater_borrowers.push((borrower.address, health_factor));
                }
            }
            info!(
                "Found {} underwater borrowers, total progress: {}%",
                underwater_borrowers.len(),
                100 * (MULTICALL_CHUNK_SIZE * i) / n,
            );
            i += 1;

            // sleep to avoid hitting rate limits
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // FIXME:
            if underwater_borrowers.len() >= 50 {
                info!("Too many underwater borrowers, stopping search");
                break;
            }
        }

        // sort borrowers by health factor
        underwater_borrowers.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(underwater_borrowers)
    }

    // load borrower state cache from file if exists
    fn load_cache(&mut self) -> Result<()> {
        match File::open(STATE_CACHE_FILE) {
            Ok(file) => {
                let cache: StateCache = match serde_json::from_reader(file) {
                    Ok(cache) => cache,
                    Err(e) => {
                        error!("Failed to parse state cache: {}", e);
                        return Err(anyhow!("Failed to parse state cache: {}", e));
                    }
                };
                info!("read state cache from file");
                self.last_block_number = cache.last_block_number;
                self.borrowers = cache.borrowers;
            }
            Err(_) => {
                info!("no state cache file found, creating new one");
                self.last_block_number = self.config.creation_block;
            }
        };

        Ok(())
    }

    // update known borrower state from last block to latest block
    async fn update_state(&mut self) -> Result<()> {
        let latest_block = self.client.get_block_number().await?;
        info!(
            "Updating state from block {} to {}",
            self.last_block_number, latest_block
        );

        self.get_borrow_logs(self.last_block_number.into(), latest_block)
            .await?
            .into_iter()
            .for_each(|log| {
                let user = log.on_behalf_of;
                // fetch assets if user already a borrower
                if self.borrowers.contains_key(&user) {
                    let borrower = self.borrowers.get_mut(&user).unwrap();
                    borrower.debt.insert(log.reserve);
                    return;
                } else {
                    self.borrowers.insert(
                        user,
                        Borrower {
                            address: user,
                            collateral: HashSet::new(),
                            debt: HashSet::from([log.reserve]),
                        },
                    );
                }
            });

        self.get_supply_logs(self.last_block_number.into(), latest_block)
            .await?
            .into_iter()
            .for_each(|log| {
                let user = log.on_behalf_of;
                // fetch assets if user already a supplier
                if self.borrowers.contains_key(&user) {
                    let borrower = self.borrowers.get_mut(&user).unwrap();
                    borrower.collateral.insert(log.reserve);
                    return;
                } else {
                    self.borrowers.insert(
                        user,
                        Borrower {
                            address: user,
                            collateral: HashSet::from([log.reserve]),
                            debt: HashSet::new(),
                        },
                    );
                }
            });

        // write state cache to file
        let cache = StateCache {
            last_block_number: latest_block.as_u64(),
            borrowers: self.borrowers.clone(),
        };
        self.last_block_number = latest_block.as_u64();
        let mut file = File::create(STATE_CACHE_FILE)?;
        file.write_all(serde_json::to_string(&cache)?.as_bytes())?;

        Ok(())
    }

    // fetch all borrow events from the from_block to to_block
    async fn get_borrow_logs(&self, from_block: U64, to_block: U64) -> Result<Vec<BorrowFilter>> {
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());

        let mut res = Vec::new();
        for start_block in
            (from_block.as_u64()..to_block.as_u64()).step_by(LOG_BLOCK_RANGE as usize)
        {
            let end_block = std::cmp::min(start_block + LOG_BLOCK_RANGE - 1, to_block.as_u64());
            pool.borrow_filter()
                .from_block(start_block)
                .to_block(end_block)
                .address(ValueOrArray::Value(self.config.pool_address))
                .query()
                .await?
                .into_iter()
                .for_each(|log| {
                    res.push(log);
                });
        }

        Ok(res)
    }

    // fetch all borrow events from the from_block to to_block
    async fn get_supply_logs(&self, from_block: U64, to_block: U64) -> Result<Vec<SupplyFilter>> {
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());

        let mut res = Vec::new();
        for start_block in
            (from_block.as_u64()..to_block.as_u64()).step_by(LOG_BLOCK_RANGE as usize)
        {
            let end_block = std::cmp::min(start_block + LOG_BLOCK_RANGE - 1, to_block.as_u64());
            pool.supply_filter()
                .from_block(start_block)
                .to_block(end_block)
                .address(ValueOrArray::Value(self.config.pool_address))
                .query()
                .await?
                .into_iter()
                .for_each(|log| {
                    res.push(log);
                });
        }

        Ok(res)
    }

    async fn approve_tokens(&mut self) -> Result<()> {
        let liquidator = Liquidator::new(self.liquidator, self.client.clone());

        let sender = self
            .client
            .default_sender()
            .ok_or(anyhow!("No connected sender"))?;
        let mut nonce = self.client.get_transaction_count(sender, None).await?;
        for token_address in self.tokens.keys() {
            let token = IERC20::new(token_address.clone(), self.client.clone());
            match self.use_aave_liquidator {
                true => {
                    match token
                        .allowance(sender, self.config.pool_address)
                        .call()
                        .await
                    {
                        Ok(allowance) => {
                            if allowance == U256::zero() {
                                match token
                                    .approve(self.config.pool_address, U256::MAX)
                                    .nonce(nonce)
                                    .send()
                                    .await
                                {
                                    Ok(_) => nonce = nonce + 1,
                                    Err(e) => {
                                        error!("approve failed: {:?}", e);
                                        return Err(anyhow!("approve failed: {:?}", e));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("allowance check failed: {:?}", e);
                            return Err(anyhow!("allowance check failed: {:?}", e));
                        }
                    }
                }
                false => {
                    match token
                        .allowance(self.liquidator, self.config.pool_address)
                        .call()
                        .await
                    {
                        Ok(allowance) => {
                            if allowance == U256::zero() {
                                match liquidator
                                    .approve_pool(*token_address)
                                    .nonce(nonce)
                                    .send()
                                    .await
                                {
                                    Ok(_) => nonce = nonce + 1,
                                    Err(e) => {
                                        error!("approve failed: {:?}", e);
                                        return Err(anyhow!("approve failed: {:?}", e));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("allowance check failed: {:?}", e);
                            return Err(anyhow!("allowance check failed: {:?}", e));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn liquidation_call(&mut self, op: &LiquidationOpportunity) -> Result<()> {
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());

        let sender = self
            .client
            .default_sender()
            .ok_or(anyhow!("No connected sender"))?;
        let nonce = self.client.get_transaction_count(sender, None).await?;
        if self.use_aave_liquidator {
            pool.liquidation_call(op.collateral, op.debt, op.borrower, op.debt_to_cover, false)
                .nonce(nonce)
                .send()
                .await?
                .await?;
        } else {
            // TODO remove unwrap once we figure out whats broken
        }

        Ok(())
    }

    async fn update_token_configs(&mut self) -> Result<()> {
        let pool_data =
            IPoolDataProvider::<M>::new(self.config.pool_data_provider, self.client.clone());
        let all_tokens = pool_data.get_all_reserves_tokens().await?;
        let all_a_tokens = pool_data.get_all_a_tokens().await?;
        info!("all_tokens: {:?}", all_tokens);
        for (token, a_token) in zip(all_tokens, all_a_tokens) {
            match pool_data
                .get_reserve_configuration_data(token.token_address)
                .await
            {
                Ok((decimals, ltv, threshold, bonus, reserve, _, _, _, _, _)) => {
                    match pool_data
                        .get_liquidation_protocol_fee(token.token_address)
                        .await
                    {
                        Ok(protocol_fee) => {
                            self.tokens.insert(
                                token.token_address,
                                TokenConfig {
                                    address: token.token_address,
                                    a_address: a_token.token_address,
                                    decimals: decimals.low_u64(),
                                    ltv: ltv.low_u64(),
                                    liquidation_threshold: threshold.low_u64(),
                                    liquidation_bonus: bonus.low_u64(),
                                    reserve_factor: reserve.low_u64(),
                                    protocol_fee: protocol_fee.low_u64(),
                                    symbol: token.symbol,
                                },
                            );
                        }
                        Err(e) => {
                            error!(
                                "Failed to get liquidation protocol fee for token {}: {}",
                                token.token_address, e
                            );
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to get reserve configuration data for token {}: {}",
                        token.token_address, e
                    );
                }
            }
        }

        Ok(())
    }

    // 8 decimals of precision
    async fn get_asset_price_eth(&self, asset: &Address, pool_state: &PoolState) -> Result<U256> {
        // 1:1 for weth
        let weth_address = self.config.weth_address;
        if asset.eq(&weth_address) {
            return Ok(U256::from(PRICE_ONE));
        }

        // usd / token
        let usd_price = pool_state
            .prices
            .get(asset)
            .ok_or(anyhow!("No price found for asset {:?}", asset))?;
        let asset_symbol = self
            .tokens
            .get(asset)
            .map(|t| t.symbol.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        info!("{}/USD: {}, asset: {:?}", asset_symbol, usd_price, asset);
        // usd / eth
        let usd_price_eth = pool_state
            .prices
            .get(&weth_address)
            .ok_or(anyhow!("No price found for asset {}", weth_address))?;
        info!("WETH/USD: {}, asset: {:?}", usd_price_eth, weth_address);
        // usd / token * eth / usd = eth / token
        let x = usd_price * U256::from(PRICE_ONE) / usd_price_eth;
        info!("{}/ETH: {}", asset_symbol, x);
        Ok(x)
    }

    async fn get_best_liquidation_op(&mut self) -> Result<Option<LiquidationOpportunity>> {
        let underwater = self.get_underwater_borrowers().await?;

        if underwater.len() == 0 {
            return Err(anyhow!("No underwater borrowers found"));
        }

        info!("Found {} underwater borrowers", underwater.len());
        let pool_data =
            IPoolDataProvider::<M>::new(self.config.pool_data_provider, self.client.clone());

        let mut best_bonus: I256 = I256::MIN;
        let mut best_op: Option<LiquidationOpportunity> = None;
        let pool_state = self.get_pool_state().await?;

        for (borrower, health_factor) in underwater {
            if let Some(op) = self
                .get_liquidation_opportunity(
                    self.borrowers
                        .get(&borrower)
                        .ok_or(anyhow!("Borrower not found"))?,
                    &pool_data,
                    &health_factor,
                    &pool_state,
                )
                .await
                .map_err(|e| info!("Liquidation op failed {}", e))
                .ok()
            {
                if op.profit_eth > best_bonus {
                    best_bonus = op.profit_eth;
                    best_op = Some(op);
                }
            }
        }

        Ok(best_op)
    }

    async fn get_pool_state(&self) -> Result<PoolState> {
        let mut multicall = Multicall::<M>::new(
            self.client.clone(),
            Some(H160::from_str(
                "0xcA11bde05977b3631167028862bE2a173976CA11",
            )?),
        )
        .await?;
        let mut prices = HashMap::new();
        let price_oracle = IAaveOracle::<M>::new(self.config.oracle_address, self.client.clone());

        for token_address in self.tokens.keys() {
            multicall.add_call(price_oracle.get_asset_price(*token_address), false);
        }

        let result: Vec<U256> = multicall.call_array().await?;
        for (token_address, price) in zip(self.tokens.keys(), result) {
            prices.insert(*token_address, price);
        }
        multicall.clear_calls();

        Ok(PoolState { prices })
    }

    async fn get_liquidation_opportunity(
        &self,
        borrower: &Borrower,
        pool_data: &IPoolDataProvider<M>,
        health_factor: &U256,
        pool_state: &PoolState,
    ) -> Result<LiquidationOpportunity> {
        let Borrower {
            address: borrower_address,
            collateral,
            debt,
        } = borrower;
        // TODO: handle users with multiple collateral / debt
        // get first item out of the set
        let collateral_address = collateral
            .iter()
            .next()
            .ok_or(anyhow!("No collateral found"))?;
        let debt_address = debt.iter().next().ok_or(anyhow!("No debt found"))?;
        let collateral_asset_price = pool_state
            .prices
            .get(collateral_address)
            .ok_or(anyhow!("No collateral price"))?;
        let debt_asset_price = pool_state
            .prices
            .get(debt_address)
            .ok_or(anyhow!("No debt price"))?;
        let collateral_config = self
            .tokens
            .get(collateral_address)
            .ok_or(anyhow!("Failed to get collateral address"))?;
        let debt_config = self
            .tokens
            .get(debt_address)
            .ok_or(anyhow!("Failed to get debt address"))?;
        let collateral_unit = U256::from(10).pow(collateral_config.decimals.into());
        let debt_unit = U256::from(10).pow(debt_config.decimals.into());
        let liquidation_bonus = collateral_config.liquidation_bonus;
        let a_token = IERC20::new(collateral_config.a_address.clone(), self.client.clone());

        let (_, stable_debt, variable_debt, _, _, _, _, _, _) = pool_data
            .get_user_reserve_data(*debt_address, *borrower_address)
            .await?;
        let close_factor = if health_factor.gt(&U256::from(LIQUIDATION_CLOSE_FACTOR_THRESHOLD)) {
            U256::from(DEFAULT_LIQUIDATION_CLOSE_FACTOR)
        } else {
            U256::from(MAX_LIQUIDATION_CLOSE_FACTOR)
        };

        let mut debt_to_cover =
            (stable_debt + variable_debt) * close_factor / MAX_LIQUIDATION_CLOSE_FACTOR;
        let base_collateral = (debt_asset_price * debt_to_cover * collateral_unit)
            / (collateral_asset_price * debt_unit);
        let mut collateral_to_liquidate = percent_mul(base_collateral, liquidation_bonus);
        let user_collateral_balance = a_token.balance_of(*borrower_address).await?;

        if collateral_to_liquidate > user_collateral_balance {
            collateral_to_liquidate = user_collateral_balance;
            debt_to_cover = (collateral_asset_price * collateral_to_liquidate * debt_unit)
                / percent_div(debt_asset_price * collateral_unit, liquidation_bonus);
        }

        let collateral_symbol = self
            .tokens
            .get(collateral_address)
            .map(|c| c.symbol.clone())
            .unwrap_or_default();
        let debt_symbol = self
            .tokens
            .get(&debt_address)
            .map(|d| d.symbol.clone())
            .unwrap_or_default();

        let mut op = LiquidationOpportunity {
            borrower: borrower_address.clone(),
            collateral: collateral_address.clone(),
            debt: debt_address.clone(),
            debt_to_cover,
            profit_eth: I256::from(0),
            collateral_symbol,
            debt_symbol,
            profit_factor: I256::from(0),
        };

        let asset_price_in_eth = self
            .get_asset_price_eth(collateral_address, pool_state)
            .await?;
        let debt_price_in_eth = self.get_asset_price_eth(debt_address, pool_state).await?;

        if self.use_aave_liquidator {
            let asset_value_in_eth = I256::from_dec_str(&asset_price_in_eth.to_string())?
                .checked_mul(I256::from_dec_str(&collateral_to_liquidate.to_string())?)
                .unwrap()
                .checked_div(I256::from_dec_str(&collateral_unit.to_string())?)
                .unwrap();
            let debt_value_in_eth = I256::from_dec_str(&debt_price_in_eth.to_string())?
                .checked_mul(I256::from_dec_str(&debt_to_cover.to_string())?)
                .unwrap()
                .checked_div(I256::from_dec_str(&debt_unit.to_string())?)
                .unwrap();
            op.profit_eth = asset_value_in_eth.checked_sub(debt_value_in_eth).unwrap();
            op.profit_factor = asset_value_in_eth
                .checked_mul(I256::from(100))
                .unwrap()
                .checked_div(debt_value_in_eth)
                .unwrap_or(I256::from(0));
            if debt_to_cover == U256::zero() {
                op.profit_eth = I256::from(0);
                op.profit_factor = I256::from(0);
                return Err(anyhow!(
                    "No debt to cover for borrower {:?}, collateral {:?}, debt {:?}",
                    borrower_address,
                    collateral_address,
                    debt_address
                ));
            }
            if collateral_address == debt_address {
                info!(
                    "Collateral and debt are the same for borrower {:?}, collateral {:?}, debt {:?}",
                    borrower_address, collateral_address, debt_address
                );
                return Err(anyhow!(
                    "Collateral and debt are the same for borrower {:?}, collateral {:?}, debt {:?}, not support yet",
                    borrower_address,
                    collateral_address,
                    debt_address
                ));
            }
            info!(
                "Using Aave liquidator - profit in ETH: {}, asset_value_in_eth: {}, debt_value_in_eth: {}, profit factor: {}%",
                op.profit_eth, asset_value_in_eth, debt_value_in_eth, op.profit_factor
            );
            self.build_liquidation(&op)
                .await
                .map_err(|e| error!("Error building liquidation: {}", e))
                .unwrap();
        } else {
            let gain = self.build_liquidation_call(&op).await?.call().await?;
            op.profit_eth =
                gain * I256::from_dec_str(&asset_price_in_eth.to_string())? / I256::from(PRICE_ONE);
        }

        info!(
            "Found opportunity - borrower: {:?}, collateral: {:?}({}), debt: {:?}({}), collateral_to_liquidate: {:?}, debt_to_cover: {:?}, profit_eth: {:?}",
            op.borrower, collateral_address, op.collateral_symbol, debt_address, op.debt_symbol, collateral_to_liquidate, debt_to_cover, op.profit_eth
        );

        Ok(op)
    }

    async fn build_liquidation_call(
        &self,
        op: &LiquidationOpportunity,
    ) -> Result<ContractCall<M, I256>> {
        if self.config.l2_encoder == Address::zero() {
            return Err(anyhow!(
                "L2 Encoder address is not deployed on this network"
            ));
        }
        let liquidator = Liquidator::new(self.liquidator, self.client.clone());
        let encoder = L2Encoder::new(self.config.l2_encoder, self.client.clone());
        let (data0, data1) = encoder
            .encode_liquidation_call(op.collateral, op.debt, op.borrower, op.debt_to_cover, false)
            .call()
            .await?;

        // TODO: handle arbitrary pool fees
        Ok(liquidator.liquidate(op.collateral, op.debt, 500, op.debt_to_cover, data0, data1))
    }

    async fn build_liquidation(
        &self,
        op: &LiquidationOpportunity,
    ) -> Result<TypedTransaction, anyhow::Error> {
        if self.use_aave_liquidator {
            let pool = Pool::new(self.config.pool_address, self.client.clone());
            let mut call =
                pool.liquidation_call(op.collateral, op.debt, op.borrower, op.debt_to_cover, false);
            Ok(call.tx.set_chain_id(self.chain_id).clone())
        } else {
            let mut call = self.build_liquidation_call(op).await?;
            Ok(call.tx.set_chain_id(self.chain_id).clone())
        }
    }
}

fn percent_mul(a: U256, bps: u64) -> U256 {
    (U256::from(5000) + (a * bps)) / U256::from(10000)
}

fn percent_div(a: U256, bps: u64) -> U256 {
    let half_bps = bps / 2;
    (U256::from(half_bps) + (a * 10000)) / bps
}
