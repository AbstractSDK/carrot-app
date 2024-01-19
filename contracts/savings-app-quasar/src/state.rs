use abstract_dex_adapter::msg::DexName;
use cosmwasm_std::{Addr, Env, MessageInfo};
use cw_asset::AssetInfo;
use cw_storage_plus::Item;

use crate::{contract::AppResult, error::AppError};

#[cosmwasm_schema::cw_serde]
pub struct Config {
    pub deposit_info: AssetInfo,
    pub quasar_pool: Addr,
    pub exchanges: Vec<DexName>,
    pub pool: cl_vault::state::PoolConfig,
}

impl Config {
    pub fn deposit_denom(&self) -> AppResult<String> {
        match &self.deposit_info {
            AssetInfo::Native(denom) => Ok(denom.clone()),
            _ => Err(AppError::WrongAssetInfo {}),
        }
    }
}

pub fn assert_contract(info: &MessageInfo, env: &Env) -> AppResult<()> {
    if info.sender == env.contract.address {
        Ok(())
    } else {
        Err(AppError::Unauthorized {})
    }
}

pub const CONFIG: Item<Config> = Item::new("config");
