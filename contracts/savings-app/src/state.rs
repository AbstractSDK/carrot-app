use abstract_app::abstract_core::objects::AssetEntry;
use abstract_dex_adapter::msg::DexName;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Deps, Env, MessageInfo};
use cw_asset::AssetInfo;
use cw_storage_plus::Item;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    ConcentratedliquidityQuerier, FullPositionBreakdown,
};

use crate::{contract::AppResult, error::AppError};

#[cw_serde]
pub struct Config {
    pub deposit_info: AssetInfo,
    pub pool_config: PoolConfig,
    pub exchange: DexName,
}

impl Config {
    pub fn deposit_denom(&self) -> AppResult<String> {
        match &self.deposit_info {
            AssetInfo::Native(denom) => Ok(denom.clone()),
            _ => Err(AppError::WrongAssetInfo {}),
        }
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
