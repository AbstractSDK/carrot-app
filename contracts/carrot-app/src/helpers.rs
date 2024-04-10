use abstract_app::{objects::AssetEntry, traits::AbstractNameService};
use abstract_sdk::Resolve;
use cosmwasm_std::{Addr, Coin, Deps, Uint128};
use osmosis_std::cosmwasm_to_proto_coins;

use crate::{
    contract::{App, AppResult},
    error::AppError,
};

pub fn get_user(deps: Deps, app: &App) -> AppResult<Addr> {
    Ok(app
        .admin
        .query_account_owner(deps)?
        .admin
        .ok_or(AppError::NoTopLevelAccount {})
        .map(|admin| deps.api.addr_validate(&admin))??)
}

pub fn get_balance(a: AssetEntry, deps: Deps, address: Addr, app: &App) -> AppResult<Uint128> {
    let denom = a.resolve(&deps.querier, &app.ans_host(deps)?)?;
    let user_gas_balance = denom.query_balance(&deps.querier, address.clone())?;
    Ok(user_gas_balance)
}

/// For passing coins inside osmosis messages they have to be sorted
pub fn cosmwasm_to_proto_coins_sorted(
    mut coins: Vec<Coin>,
) -> Vec<osmosis_std::types::cosmos::base::v1beta1::Coin> {
    coins.sort_by(|a, b| a.denom.cmp(&b.denom));
    cosmwasm_to_proto_coins(coins)
}
