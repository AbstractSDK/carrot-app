use abstract_dex_adapter::msg::DexName;
use cosmwasm_std::Addr;
use cw_asset::AssetInfo;
use cw_storage_plus::Item;

use crate::{contract::AppResult, error::AppError};

#[cosmwasm_schema::cw_serde]
pub struct Config {
    pub deposit_info: AssetInfo,
    pub quasar_pool: Addr,
    pub exchanges: Vec<DexName>,
}

impl Config {
    pub fn deposit_denom(&self) -> AppResult<String> {
        match &self.deposit_info {
            AssetInfo::Native(denom) => Ok(denom.clone()),
            _ => Err(AppError::WrongAssetInfo {}),
        }
    }
}

pub const CONFIG: Item<Config> = Item::new("config");
