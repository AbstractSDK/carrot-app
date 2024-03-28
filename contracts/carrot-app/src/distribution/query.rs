use cosmwasm_std::{Deps, Uint128};

use crate::{
    contract::{App, AppResult},
    error::AppError,
    handlers::query::query_exchange_rate,
    msg::AssetsBalanceResponse,
};

impl AssetsBalanceResponse {
    pub fn value(&self, deps: Deps, app: &App) -> AppResult<Uint128> {
        self.balances
            .iter()
            .map(|balance| {
                let exchange_rate = query_exchange_rate(deps, &balance.denom, app)?;

                Ok::<_, AppError>(exchange_rate * balance.amount)
            })
            .sum()
    }
}
