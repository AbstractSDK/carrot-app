use crate::contract::{App, AppResult};
use abstract_app::traits::AccountIdentification;
use abstract_app::{objects::AssetEntry, traits::AbstractNameService};
use abstract_sdk::Resolve;
use cosmwasm_std::{Addr, Coin, Coins, Deps, StdResult, Uint128};

pub fn get_balance(a: AssetEntry, deps: Deps, address: Addr, app: &App) -> AppResult<Uint128> {
    let denom = a.resolve(&deps.querier, &app.ans_host(deps)?)?;
    let user_gas_balance = denom.query_balance(&deps.querier, address.clone())?;
    Ok(user_gas_balance)
}

pub fn get_proxy_balance(deps: Deps, app: &App, denom: String) -> AppResult<Coin> {
    Ok(deps
        .querier
        .query_balance(app.account_base(deps)?.proxy, denom.clone())?)
}

pub fn add_funds(funds: Vec<Coin>, to_add: Coin) -> StdResult<Vec<Coin>> {
    let mut funds: Coins = funds.try_into()?;
    funds.add(to_add)?;
    Ok(funds.into())
}
