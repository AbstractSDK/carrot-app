use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Uint128};
use cw_storage_plus::Item;

use crate::autocompound::{AutocompoundConfigBase, AutocompoundState};
use crate::yield_sources::{BalanceStrategyBase, Checkable, Checked, Unchecked};

pub const CONFIG: Item<Config> = Item::new("config");
pub const AUTOCOMPOUND_STATE: Item<AutocompoundState> = Item::new("position");
pub const CURRENT_EXECUTOR: Item<Addr> = Item::new("executor");

// TEMP VARIABLES FOR DEPOSITING INTO ONE STRATEGY
pub const TEMP_CURRENT_COIN: Item<Coin> = Item::new("temp_current_coins");
pub const TEMP_EXPECTED_SWAP_COIN: Item<Uint128> = Item::new("temp_expected_swap_coin");
pub const TEMP_DEPOSIT_COINS: Item<Vec<Coin>> = Item::new("temp_deposit_coins");
pub const TEMP_CURRENT_YIELD: Item<usize> = Item::new("temp_current_yield_type");

pub type Config = ConfigBase<Checked>;
pub type ConfigUnchecked = ConfigBase<Unchecked>;

impl From<Config> for ConfigUnchecked {
    fn from(value: Config) -> Self {
        Self {
            balance_strategy: value.balance_strategy.into(),
            autocompound_config: value.autocompound_config.into(),
            dex: value.dex,
        }
    }
}

#[cw_serde]
pub struct ConfigBase<T> {
    pub balance_strategy: BalanceStrategyBase<T>,
    pub autocompound_config: AutocompoundConfigBase<T>,
    pub dex: String,
}

impl Checkable for ConfigUnchecked {
    type CheckOutput = Config;

    fn check(
        self,
        deps: cosmwasm_std::Deps,
        app: &crate::contract::App,
    ) -> crate::contract::AppResult<Self::CheckOutput> {
        Ok(Config {
            balance_strategy: self.balance_strategy.check(deps, app)?,
            autocompound_config: AutocompoundConfigBase {
                cooldown_seconds: self.autocompound_config.cooldown_seconds,
                rewards: self
                    .autocompound_config
                    .rewards
                    .check(deps, app, &self.dex)?,
            },
            dex: self.dex,
        })
    }
}
