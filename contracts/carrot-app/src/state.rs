use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Deps, DepsMut, StdResult, Uint128};
use cw_storage_plus::Item;

use crate::autocompound::{AutocompoundConfigBase, AutocompoundState};
use crate::check::{Checked, Unchecked};
use crate::contract::AppResult;
use crate::yield_sources::Strategy;

pub const CONFIG: Item<Config> = Item::new("config");
/// Don't make this config public to avoid saving the cache inside the struct directly
const STRATEGY_CONFIG: Item<Strategy> = Item::new("strategy_config");
pub const AUTOCOMPOUND_STATE: Item<AutocompoundState> = Item::new("position");
pub const CURRENT_EXECUTOR: Item<Addr> = Item::new("executor");

// TEMP VARIABLES FOR DEPOSITING INTO ONE STRATEGY
pub const TEMP_CURRENT_COIN: Item<Coin> = Item::new("temp_current_coins");
pub const TEMP_EXPECTED_SWAP_COIN: Item<Uint128> = Item::new("temp_expected_swap_coin");
pub const TEMP_DEPOSIT_COINS: Item<Vec<Coin>> = Item::new("temp_deposit_coins");
pub const TEMP_CURRENT_YIELD: Item<usize> = Item::new("temp_current_yield_type");

pub type Config = ConfigBase<Checked>;
pub type ConfigUnchecked = ConfigBase<Unchecked>;

#[cw_serde]
pub struct ConfigBase<T> {
    pub autocompound_config: AutocompoundConfigBase<T>,
    pub dex: String,
}

pub fn load_strategy(deps: Deps) -> StdResult<Strategy> {
    load_strategy(deps)
}

pub fn save_strategy(deps: DepsMut, strategy: &mut Strategy) -> AppResult<()> {
    // We need to correct positions for which the cache is not empty
    // This is a security measure
    strategy
        .0
        .iter_mut()
        .for_each(|s| s.yield_source.params.clear_cache());
    STRATEGY_CONFIG.save(deps.storage, &strategy)?;
    Ok(())
}
