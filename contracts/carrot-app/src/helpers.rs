use abstract_app::{objects::AssetEntry, traits::AbstractNameService};
use abstract_sdk::Resolve;
use cosmwasm_std::{Addr, Deps, MessageInfo, Uint128};

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

/// Copy of [`cw_utils::nonpayable`] but with custom error type
pub fn nonpayable(info: &MessageInfo) -> AppResult<()> {
    if info.funds.is_empty() {
        Ok(())
    } else {
        Err(AppError::RedundantFunds {})
    }
}
