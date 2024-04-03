use abstract_app::abstract_sdk::{feature_objects::AnsHost, Resolve};
use abstract_app::{abstract_core::objects::AssetEntry, objects::DexAssetPairing};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{ensure, Addr, Deps, Env, MessageInfo, Timestamp, Uint128, Uint64};
use cw_storage_plus::Item;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    ConcentratedliquidityQuerier, FullPositionBreakdown,
};

use crate::{contract::AppResult, error::AppError, msg::CompoundStatus};

pub const CONFIG: Item<Config> = Item::new("config2");
pub const POSITION: Item<Position> = Item::new("position");
pub const CURRENT_EXECUTOR: Item<Addr> = Item::new("executor");

#[cw_serde]
pub struct Config {
    pub pool_config: PoolConfig,
    pub autocompound_cooldown_seconds: Uint64,
    pub autocompound_rewards_config: AutocompoundRewardsConfig,
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

#[cw_serde]
pub struct Position {
    pub owner: Addr,
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

    Ok(ConcentratedliquidityQuerier::new(&deps.querier)
        .position_by_id(position.position_id)?
        .position
        .unwrap())
}

/// Returns compound status and position
pub fn get_position_status(
    deps: Deps,
    env: &Env,
    cooldown_seconds: u64,
) -> AppResult<(CompoundStatus, Option<FullPositionBreakdown>)> {
    let position = POSITION.may_load(deps.storage)?;
    let status = match position {
        Some(position) => {
            let Ok(position_response) = ConcentratedliquidityQuerier::new(&deps.querier)
                .position_by_id(position.position_id)
            else {
                return Ok((
                    CompoundStatus::PositionNotAvailable(position.position_id),
                    None,
                ));
            };
            let ready_on = position.last_compound.plus_seconds(cooldown_seconds);
            if env.block.time >= ready_on {
                (CompoundStatus::Ready {}, position_response.position)
            } else {
                (
                    CompoundStatus::Cooldown(
                        (env.block.time.seconds() - ready_on.seconds()).into(),
                    ),
                    position_response.position,
                )
            }
        }
        None => (CompoundStatus::NoPosition {}, None),
    };
    Ok(status)
}
