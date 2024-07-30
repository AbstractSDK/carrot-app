use abstract_app::{objects::AssetEntry, sdk::AbstractResponse};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{DepsMut, Env, Uint64};
use cw_storage_plus::Item;

use crate::{
    contract::{App, AppResult},
    msg::AppMigrateMsg,
    state::{AutocompoundRewardsConfig, CarrotPosition, Config, PoolConfig, CONFIG},
};

const V0_1CONFIG: Item<V0_1Config> = Item::new("config");
const V0_1POSITION: Item<V0_1Position> = Item::new("position");

#[cw_serde]
pub struct V0_1Config {
    pub pool_config: V0_1PoolConfig,
    pub autocompound_cooldown_seconds: Uint64,
    pub autocompound_rewards_config: AutocompoundRewardsConfig,
}

#[cw_serde]
pub struct V0_1PoolConfig {
    pub pool_id: u64,
    pub token0: String,
    pub token1: String,
    pub asset0: AssetEntry,
    pub asset1: AssetEntry,
}

#[cw_serde]
pub struct V0_1Position {
    pub owner: cosmwasm_std::Addr,
    pub position_id: u64,
    pub last_compound: cosmwasm_std::Timestamp,
}

/// Handle the app migrate msg
/// The top-level Abstract app does version checking and dispatches to this handler
pub fn migrate_handler(deps: DepsMut, mut env: Env, app: App, _msg: AppMigrateMsg) -> AppResult {
    // Migrate old config
    let maybe_old_config = V0_1CONFIG.may_load(deps.storage)?;
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
        V0_1CONFIG.remove(deps.storage);
    }
    if let Some(old_position) = V0_1POSITION.may_load(deps.storage)? {
        // save_position uses ENV for determining time, so need to trick it here a little
        env.block.time = old_position.last_compound;
        CarrotPosition::save_position(
            deps.storage,
            &old_position.last_compound,
            old_position.position_id,
        )?;
        V0_1POSITION.remove(deps.storage);
    }

    Ok(app.response("migrate"))
}
