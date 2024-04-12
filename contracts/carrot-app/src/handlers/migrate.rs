use abstract_app::{abstract_sdk::AbstractResponse, objects::AssetEntry};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{DepsMut, Env, Uint64};
use cw_storage_plus::Item;

use crate::{
    contract::{App, AppResult},
    msg::AppMigrateMsg,
    state::{AutocompoundRewardsConfig, CarrotPosition, Config, PoolConfig, CONFIG},
};

const OLD_CONFIG: Item<OldConfig> = Item::new("config");
const OLD_POSITION: Item<OldPosition> = Item::new("position");

#[cw_serde]
pub struct OldConfig {
    pub pool_config: OldPoolConfig,
    pub autocompound_cooldown_seconds: Uint64,
    pub autocompound_rewards_config: AutocompoundRewardsConfig,
}

#[cw_serde]
pub struct OldPoolConfig {
    pub pool_id: u64,
    pub token0: String,
    pub token1: String,
    pub asset0: AssetEntry,
    pub asset1: AssetEntry,
}

#[cw_serde]
pub struct OldPosition {
    pub owner: cosmwasm_std::Addr,
    pub position_id: u64,
    pub last_compound: cosmwasm_std::Timestamp,
}

/// Handle the app migrate msg
/// The top-level Abstract app does version checking and dispatches to this handler
pub fn migrate_handler(deps: DepsMut, mut env: Env, app: App, _msg: AppMigrateMsg) -> AppResult {
    // Migrate old config
    let maybe_old_config = OLD_CONFIG.may_load(deps.storage)?;
    if let Some(old_config) = maybe_old_config {
        let new_config = Config {
            pool_config: PoolConfig {
                pool_id: old_config.pool_config.pool_id,
                asset0: old_config.pool_config.asset0,
                asset1: old_config.pool_config.asset1,
            },
            autocompound_cooldown_seconds: old_config.autocompound_cooldown_seconds,
            autocompound_rewards_config: old_config.autocompound_rewards_config,
        };
        CONFIG.save(deps.storage, &new_config)?;
        OLD_CONFIG.remove(deps.storage);
    }
    if let Some(old_position) = OLD_POSITION.may_load(deps.storage)? {
        // save_position uses ENV for determining time, so need to trick it here a little
        env.block.time = old_position.last_compound;
        CarrotPosition::save_position(
            deps.storage,
            &old_position.last_compound,
            old_position.position_id,
        )?;
        OLD_POSITION.remove(deps.storage);
    }

    Ok(app.response("migrate"))
}
