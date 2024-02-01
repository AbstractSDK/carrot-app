use std::collections::HashSet;

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

    // We query the dex that is accepted to swap the assets
    let exchange_strs: HashSet<&str> = msg.exchanges.iter().map(AsRef::as_ref).collect();
    let pair = asset_pairing_resp
        .into_iter()
        .find(|(pair, refs)| !refs.is_empty() && exchange_strs.contains(pair.dex()))
        .ok_or(AppError::NoSwapPossibility {})?
        .0;
    let dex_name = pair.dex();

    let config: Config = Config {
        deposit_info: cw_asset::AssetInfoBase::Native(msg.deposit_denom),
        exchange: dex_name.to_string(),
        pool_config: PoolConfig {
            pool_id: msg.pool_id,
            token0: pool.token0.clone(),
            token1: pool.token1.clone(),
            asset0,
            asset1,
        },
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
