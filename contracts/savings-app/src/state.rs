use abstract_app::abstract_sdk::{feature_objects::AnsHost, Resolve};
use abstract_app::{abstract_core::objects::AssetEntry, objects::DexAssetPairing};
use abstract_dex_adapter::msg::DexName;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{ensure, Deps, Env, MessageInfo, Storage, Timestamp, Uint128, Uint64};
use cw_asset::AssetInfo;
use cw_storage_plus::Item;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    ConcentratedliquidityQuerier, FullPositionBreakdown,
};

use crate::{contract::AppResult, error::AppError, msg::CompoundStatus};

#[cw_serde]
pub struct Config {
    pub deposit_info: AssetInfo,
    pub pool_config: PoolConfig,
    pub exchange: DexName,
    pub autocompound_cooldown_seconds: Uint64,
    pub autocompound_rewards_config: AutocompoundRewardsConfig,
}

impl Config {
    pub fn deposit_denom(&self) -> AppResult<String> {
        match &self.deposit_info {
            AssetInfo::Native(denom) => Ok(denom.clone()),
            _ => Err(AppError::WrongAssetInfo {}),
        }
    }
}

/// Configuration on how rewards should be distributed
/// to the address who helped to execute autocompound
#[cw_serde]
pub struct AutocompoundRewardsConfig {
    /// Gas denominator for this chain
    pub gas_denom: String,
    /// Denominator of the asset that will be used for swap to the gas asset
    pub swap_denom: String,
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
        let gas_asset =
            AssetInfo::Native(self.gas_denom.clone()).resolve(&deps.querier, ans_host)?;
        let swap_asset =
            AssetInfo::Native(self.swap_denom.clone()).resolve(&deps.querier, ans_host)?;

        DexAssetPairing::new(gas_asset, swap_asset, dex_name).resolve(&deps.querier, ans_host)?;

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

pub fn assert_contract(info: &MessageInfo, env: &Env) -> AppResult<()> {
    if info.sender == env.contract.address {
        Ok(())
    } else {
        Err(AppError::Unauthorized {})
    }
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const POSITION: Item<Position> = Item::new("position");

#[cw_serde]
pub struct Position {
    pub owner: String,
    pub position_id: u64,
    pub last_compound: Timestamp,
}

pub fn get_position(deps: Deps) -> AppResult<Position> {
    POSITION
        .load(deps.storage)
        .map_err(|_| AppError::NoPosition {})
}

pub fn get_osmosis_position(deps: Deps) -> AppResult<FullPositionBreakdown> {
    let position = get_position(deps)?;

    ConcentratedliquidityQuerier::new(&deps.querier)
        .position_by_id(position.position_id)
        .map_err(|e| {
            cosmwasm_std::StdError::generic_err(format!(
                "Failed to query position by id: {}\n error: {e}",
                position.position_id
            ))
        })?
        .position
        .ok_or(AppError::NoPosition {})
}

pub fn get_position_status(
    storage: &dyn Storage,
    env: &Env,
    cooldown_secods: u64,
) -> AppResult<CompoundStatus> {
    let position = POSITION.may_load(storage)?;
    let status = match position {
        Some(position) => {
            let ready_on = position.last_compound.plus_seconds(cooldown_secods);
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
