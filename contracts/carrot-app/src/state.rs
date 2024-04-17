use abstract_app::objects::AnsAsset;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Item;

use crate::autocompound::{AutocompoundConfigBase, AutocompoundState};
use crate::check::{Checked, Unchecked};
use crate::yield_sources::Strategy;

pub const CONFIG: Item<Config> = Item::new("config");
pub const STRATEGY_CONFIG: Item<Strategy> = Item::new("strategy_config");
pub const AUTOCOMPOUND_STATE: Item<AutocompoundState> = Item::new("position");
pub const CURRENT_EXECUTOR: Item<Addr> = Item::new("executor");

// TEMP VARIABLES FOR DEPOSITING INTO ONE STRATEGY
pub const TEMP_CURRENT_ASSET: Item<AnsAsset> = Item::new("temp_current_asset");
pub const TEMP_EXPECTED_SWAP_COIN: Item<Uint128> = Item::new("temp_expected_swap_coin");
pub const TEMP_DEPOSIT_ASSETS: Item<Vec<AnsAsset>> = Item::new("temp_deposit_assets");
pub const TEMP_CURRENT_YIELD: Item<usize> = Item::new("temp_current_yield_type");

pub type Config = ConfigBase<Checked>;
pub type ConfigUnchecked = ConfigBase<Unchecked>;

#[cw_serde]
pub struct ConfigBase<T> {
    pub autocompound_config: AutocompoundConfigBase<T>,
    pub dex: String,
}
