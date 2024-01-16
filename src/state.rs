use cosmwasm_std::{Addr};
use cw_asset::AssetInfo;
use cw_storage_plus::Item;

use crate::{contract::AppResult, error::AppError};

#[cosmwasm_schema::cw_serde]
pub struct Config {
    pub deposit_info: AssetInfo,
    pub quasar_pool: Addr,
}

impl Config {
    pub fn deposit_denom(&self) -> AppResult<String> {
        match &self.deposit_info {
            AssetInfo::Native(denom) => Ok(denom.clone()),
            _ => Err(AppError::WrongAssetInfo {}),
        }
    }
}

#[cosmwasm_schema::cw_serde]
pub struct State {
    pub current_position_id: Option<u64>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const STATE: Item<State> = Item::new("state");
