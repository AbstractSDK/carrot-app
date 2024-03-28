use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Uint128};
use cw_storage_plus::Item;

use crate::autocompound::{AutocompoundConfig, AutocompoundState};
use crate::yield_sources::BalanceStrategy;

pub const CONFIG: Item<Config> = Item::new("config");
pub const AUTOCOMPOUND_STATE: Item<AutocompoundState> = Item::new("position");
pub const CURRENT_EXECUTOR: Item<Addr> = Item::new("executor");

// TEMP VARIABLES FOR DEPOSITING INTO ONE STRATEGY
pub const TEMP_CURRENT_COIN: Item<Coin> = Item::new("temp_current_coins");
pub const TEMP_EXPECTED_SWAP_COIN: Item<Uint128> = Item::new("temp_expected_swap_coin");
pub const TEMP_DEPOSIT_COINS: Item<Vec<Coin>> = Item::new("temp_deposit_coins");
pub const TEMP_CURRENT_YIELD: Item<usize> = Item::new("temp_current_yield_type");

#[cw_serde]
pub struct Config {
    pub balance_strategy: BalanceStrategy,
    pub autocompound_config: AutocompoundConfig,
    pub dex: String,
}
