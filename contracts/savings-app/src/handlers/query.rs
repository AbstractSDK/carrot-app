use crate::contract::{App, AppResult};
use crate::msg::{AppQueryMsg, AssetsBalanceResponse, AvailableRewardsResponse};
use crate::state::{get_osmosis_position, Config, CONFIG};
use cosmwasm_std::{to_json_binary, Binary, Deps, Env};
use osmosis_std::try_proto_to_cosmwasm_coins;

pub fn query_handler(deps: Deps, _env: Env, app: &App, msg: AppQueryMsg) -> AppResult<Binary> {
    match msg {
        AppQueryMsg::Balance {} => to_json_binary(&query_balance(deps, app)?),
        AppQueryMsg::AvailableRewards {} => to_json_binary(&query_rewards(deps, app)?),
        AppQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
    }
    .map_err(Into::into)
}

fn query_config(deps: Deps) -> AppResult<Config> {
    Ok(CONFIG.load(deps.storage)?)
}
fn query_balance(deps: Deps, _app: &App) -> AppResult<AssetsBalanceResponse> {
    let pool = get_osmosis_position(deps)?;

    let balances = try_proto_to_cosmwasm_coins(vec![pool.asset0.unwrap(), pool.asset1.unwrap()])?;

    Ok(AssetsBalanceResponse { balances })
}

fn query_rewards(deps: Deps, _app: &App) -> AppResult<AvailableRewardsResponse> {
    let pool = get_osmosis_position(deps)?;

    // TODO make sure we merge them, so that there are no collisions
    let available_rewards = try_proto_to_cosmwasm_coins(
        pool.claimable_incentives
            .into_iter()
            .chain(pool.claimable_spread_rewards),
    )?;

    Ok(AvailableRewardsResponse { available_rewards })
}

#[derive(Debug)]
pub struct ContractBalances<T> {
    pub token0: T,
    pub token1: T,
}
