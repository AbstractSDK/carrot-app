use crate::cl_vault::{self};
use crate::contract::{App, AppResult};
use crate::msg::{AppQueryMsg, AssetsBalanceResponse, AvailableRewardsResponse, StateResponse};
use crate::state::CONFIG;
use abstract_sdk::features::AccountIdentification;
use cl_vault::msg::{ExtensionQueryMsg, QueryMsg, UserBalanceQueryMsg};
use cl_vault::query::UserRewardsResponse;
use cosmwasm_std::{to_json_binary, Binary, Deps, Env, StdResult, Uint128};

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

fn query_balance(deps: Deps, app: &App) -> AppResult<AssetsBalanceResponse> {
    let config = CONFIG.load(deps.storage)?;

    Ok(deps.querier.query_wasm_smart(
        config.quasar_pool.to_string(),
        &QueryMsg::VaultExtension(ExtensionQueryMsg::Balances(
            UserBalanceQueryMsg::UserAssetsBalance {
                user: app.proxy_address(deps)?.to_string(),
            },
        )),
    )?)
}
fn query_rewards(deps: Deps, app: &App) -> AppResult<AvailableRewardsResponse> {
    let config = CONFIG.load(deps.storage)?;

    let response: UserRewardsResponse = deps.querier.query_wasm_smart(
        config.quasar_pool.to_string(),
        &QueryMsg::VaultExtension(ExtensionQueryMsg::Balances(
            UserBalanceQueryMsg::UserRewards {
                user: app.proxy_address(deps)?.to_string(),
            },
        )),
    )?;
    Ok(AvailableRewardsResponse {
        available_rewards: response.rewards,
    })
}

pub fn query_balances(
    deps: Deps,
    app: &App,
    token0: &str,
    token1: &str,
) -> AppResult<ContractBalances<Uint128>> {
    let addr = app.proxy_address(deps)?;

    let asset0_balance = deps.querier.query_balance(addr.clone(), token0)?;
    let asset1_balance = deps.querier.query_balance(addr, token1)?;

    Ok(ContractBalances {
        token0: asset0_balance.amount,
        token1: asset1_balance.amount,
    })
}

pub struct ContractBalances<T> {
    pub token0: T,
    pub token1: T,
}
