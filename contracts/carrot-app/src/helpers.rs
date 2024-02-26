use cosmwasm_std::{Addr, Deps};

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
