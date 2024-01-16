use crate::contract::{App, AppResult};
use crate::msg::{AppQueryMsg, AvailableRewardsResponse, StateResponse};
use crate::state::CONFIG;
use cosmwasm_std::{
    coin, coins, to_json_binary, BalanceResponse, Binary, Deps, Env, StdResult, Uint128,
};

pub fn query_handler(deps: Deps, _env: Env, app: &App, msg: AppQueryMsg) -> AppResult<Binary> {
    match msg {
        AppQueryMsg::State {} => to_json_binary(&query_state(deps)?),
        AppQueryMsg::Balance {} => to_json_binary(&query_balance(deps, app)?),
        AppQueryMsg::AvailableRewards {} => to_json_binary(&query_rewards(deps, app)?),
    }
    .map_err(Into::into)
}

fn query_state(deps: Deps) -> StdResult<StateResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(StateResponse {
        deposit_info: config.deposit_info.into(),
        quasar_pool: config.quasar_pool.to_string(),
        exchanges: config.exchanges,
    })
}

fn query_balance(deps: Deps, app: &App) -> StdResult<BalanceResponse> {
    let config = CONFIG.load(deps.storage)?;

    Ok(BalanceResponse {
        amount: coin(0, config.deposit_info.to_string()),
    })
}
fn query_rewards(deps: Deps, app: &App) -> StdResult<AvailableRewardsResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(AvailableRewardsResponse {
        available_rewards: coins(0, config.deposit_info.to_string()),
    })
}

pub fn query_balances(
    deps: Deps,
    env: &Env,
    token0: &str,
    token1: &str,
) -> AppResult<ContractBalances<Uint128>> {
    let asset0_balance = deps
        .querier
        .query_balance(env.contract.address.clone(), token0)?;
    let asset1_balance = deps
        .querier
        .query_balance(env.contract.address.clone(), token1)?;

    Ok(ContractBalances {
        token0: asset0_balance.amount,
        token1: asset1_balance.amount,
    })
}

pub struct ContractBalances<T> {
    pub token0: T,
    pub token1: T,
}
