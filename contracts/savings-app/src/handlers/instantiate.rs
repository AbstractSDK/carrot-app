use abstract_app::abstract_core::ans_host::{AssetPairingFilter, AssetPairingMapEntry};
use abstract_app::abstract_sdk::{features::AbstractNameService, AbstractResponse};
use cosmwasm_std::{DepsMut, Env, MessageInfo};
use cw_asset::AssetInfo;
use osmosis_std::types::osmosis::{
    concentratedliquidity::v1beta1::Pool, poolmanager::v1beta1::PoolmanagerQuerier,
};

use crate::{
    contract::{App, AppResult},
    error::AppError,
    msg::AppInstantiateMsg,
    state::{Config, PoolConfig, CONFIG},
};

use super::execute::_create_position;

pub fn instantiate_handler(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    app: App,
    msg: AppInstantiateMsg,
) -> AppResult {
    let pool: Pool = PoolmanagerQuerier::new(&deps.querier)
        .pool(msg.pool_id)?
        .pool
        .unwrap()
        .try_into()?;

    // We query the ANS for useful information on the tokens and pool
    let ans = app.name_service(deps.as_ref());
    // ANS Asset entries to indentify the assets inside Abstract
    let asset_entries = ans.query(&vec![
        AssetInfo::Native(pool.token0.clone()),
        AssetInfo::Native(pool.token1.clone()),
    ])?;
    let asset0 = asset_entries[0].clone();
    let asset1 = asset_entries[1].clone();
    let asset_pairing_resp: Vec<AssetPairingMapEntry> = ans.pool_list(
        Some(AssetPairingFilter {
            asset_pair: Some((asset0.clone(), asset1.clone())),
            dex: None,
        }),
        None,
        None,
    )?;

    let pair = asset_pairing_resp
        .into_iter()
        .find(|(_, refs)| !refs.is_empty())
        .ok_or(AppError::NoSwapPossibility {})?
        .0;
    let dex_name = pair.dex();

    let autocompound_rewards_config = msg.autocompound_rewards_config;
    // Check validity of autocompound rewards
    autocompound_rewards_config.check(deps.as_ref(), dex_name, ans.host())?;

    let config: Config = Config {
        pool_config: PoolConfig {
            pool_id: msg.pool_id,
            token0: pool.token0.clone(),
            token1: pool.token1.clone(),
            asset0,
            asset1,
        },
        autocompound_cooldown_seconds: msg.autocompound_cooldown_seconds,
        autocompound_rewards_config,
    };
    CONFIG.save(deps.storage, &config)?;

    let mut response = app.response("instantiate_savings_app");

    // If provided - create position
    if let Some(create_position_msg) = msg.create_position {
        let (swap_msgs, create_msg) =
            _create_position(deps.as_ref(), &env, &app, create_position_msg)?;
        response = response.add_messages(swap_msgs).add_submessage(create_msg);
    }
    Ok(response)
}
