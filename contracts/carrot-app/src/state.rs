use std::collections::HashMap;

use abstract_app::abstract_sdk::{feature_objects::AnsHost, Resolve};
use abstract_app::{abstract_core::objects::AssetEntry, objects::DexAssetPairing};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    ensure, Addr, Coin, Decimal, Deps, Env, MessageInfo, Storage, Timestamp, Uint128, Uint64,
};
use cw_storage_plus::Item;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    ConcentratedliquidityQuerier, FullPositionBreakdown,
};

use crate::yield_sources::osmosis_cl_pool::OsmosisPosition;
use crate::yield_sources::BalanceStrategy;
use crate::{contract::AppResult, error::AppError, msg::CompoundStatus};

pub const CONFIG: Item<Config> = Item::new("config");
pub const POSITION: Item<AutocompoundState> = Item::new("position");
pub const CURRENT_EXECUTOR: Item<Addr> = Item::new("executor");

// TEMP VARIABLES FOR DEPOSITING INTO ONE STRATEGY
pub const TEMP_CURRENT_COIN: Item<Coin> = Item::new("temp_current_coins");
pub const TEMP_EXPECTED_SWAP_COIN: Item<Uint128> = Item::new("temp_expected_swap_coin");
pub const TEMP_DEPOSIT_COINS: Item<Vec<Coin>> = Item::new("temp_deposit_coins");

// Storage for each yield source
pub const OSMOSIS_POSITION: Item<OsmosisPosition> = Item::new("osmosis_cl_position");

#[cw_serde]
pub struct Config {
    pub balance_strategy: BalanceStrategy,
    pub autocompound_config: AutocompoundConfig,
    pub dex: String,
}

/// General auto-compound parameters.
/// Includes the cool down and the technical funds config
#[cw_serde]
pub struct AutocompoundConfig {
    /// Seconds to wait before autocompound is incentivized.
    /// Allows the user to configure when the auto-compound happens
    pub cooldown_seconds: Uint64,
    /// Configuration of rewards to the address who helped to execute autocompound
    pub rewards: AutocompoundRewardsConfig,
}

/// Configuration on how rewards should be distributed
/// to the address who helped to execute autocompound
#[cw_serde]
pub struct AutocompoundRewardsConfig {
    /// Gas denominator for this chain
    pub gas_asset: AssetEntry,
    /// Denominator of the asset that will be used for swap to the gas asset
    pub swap_asset: AssetEntry,
    /// Reward amount
    pub reward: Uint128,
    /// If gas token balance falls below this bound a swap will be generated
    pub min_gas_balance: Uint128,
    /// Upper bound of gas tokens expected after the swap
    pub max_gas_balance: Uint128,
}

impl AutocompoundRewardsConfig {
    pub fn check(&self, deps: Deps, dex_name: &str, ans_host: &AnsHost) -> AppResult<()> {
        ensure!(
            self.reward <= self.min_gas_balance,
            AppError::RewardConfigError(
                "reward should be lower or equal to the min_gas_balance".to_owned()
            )
        );
        ensure!(
            self.max_gas_balance > self.min_gas_balance,
            AppError::RewardConfigError(
                "max_gas_balance has to be bigger than min_gas_balance".to_owned()
            )
        );

        // Check swap asset has pairing into gas asset
        DexAssetPairing::new(self.gas_asset.clone(), self.swap_asset.clone(), dex_name)
            .resolve(&deps.querier, ans_host)?;

        Ok(())
    }
}

#[cw_serde]
pub struct PoolConfig {
    pub pool_id: u64,
    pub token0: String,
    pub token1: String,
    pub asset0: AssetEntry,
    pub asset1: AssetEntry,
}

pub fn compute_total_value(
    funds: &[Coin],
    exchange_rates: &HashMap<String, Decimal>,
) -> AppResult<Uint128> {
    funds
        .iter()
        .map(|c| {
            let exchange_rate = exchange_rates
                .get(&c.denom)
                .ok_or(AppError::NoExchangeRate(c.denom.clone()))?;
            Ok(c.amount * *exchange_rate)
        })
        .sum()
}

pub fn assert_contract(info: &MessageInfo, env: &Env) -> AppResult<()> {
    if info.sender == env.contract.address {
        Ok(())
    } else {
        Err(AppError::Unauthorized {})
    }
}

#[cw_serde]
pub struct AutocompoundState {
    pub last_compound: Timestamp,
}

pub fn get_autocompound_status(
    storage: &dyn Storage,
    env: &Env,
    cooldown_seconds: u64,
) -> AppResult<CompoundStatus> {
    let position = POSITION.may_load(storage)?;
    let status = match position {
        Some(position) => {
            let ready_on = position.last_compound.plus_seconds(cooldown_seconds);
            if env.block.time >= ready_on {
                CompoundStatus::Ready {}
            } else {
                CompoundStatus::Cooldown((env.block.time.seconds() - ready_on.seconds()).into())
            }
        }
        None => CompoundStatus::NoPosition {},
    };
    Ok(status)
}
