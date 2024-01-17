use crate::cl_vault::{self, BalancesQuery, UserRewardsResponse};
use crate::contract::{App, AppResult};
use crate::msg::{AppQueryMsg, AssetsBalanceResponse, AvailableRewardsResponse, StateResponse};
use crate::state::CONFIG;
use cosmwasm_std::{to_json_binary, Binary, Deps, Env, StdResult, Uint128};

pub fn query_handler(deps: Deps, env: Env, _app: &App, msg: AppQueryMsg) -> AppResult<Binary> {
    match msg {
        AppQueryMsg::State {} => to_json_binary(&query_state(deps)?),
        AppQueryMsg::Balance {} => to_json_binary(&query_balance(deps, env)?),
        AppQueryMsg::AvailableRewards {} => to_json_binary(&query_rewards(deps, env)?),
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

fn query_balance(deps: Deps, env: Env) -> StdResult<AssetsBalanceResponse> {
    let config = CONFIG.load(deps.storage)?;

    deps.querier.query_wasm_smart(
        config.quasar_pool.to_string(),
        &cl_vault::QueryMsg::VaultExtension(cl_vault::VaultQuery::Balances(
            BalancesQuery::UserAssetsBalance {
                user: env.contract.address.to_string(),
            },
        )),
    )
}
fn query_rewards(deps: Deps, env: Env) -> StdResult<AvailableRewardsResponse> {
    let config = CONFIG.load(deps.storage)?;

    let response: UserRewardsResponse = deps.querier.query_wasm_smart(
        config.quasar_pool.to_string(),
        &cl_vault::QueryMsg::VaultExtension(cl_vault::VaultQuery::Balances(
            BalancesQuery::UserRewards {
                user: env.contract.address.to_string(),
            },
        )),
    )?;
    Ok(AvailableRewardsResponse {
        available_rewards: response.rewards,
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
