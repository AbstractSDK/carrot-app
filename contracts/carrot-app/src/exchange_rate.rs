use std::collections::HashMap;

use cosmwasm_std::{Decimal, Deps};

use crate::contract::{App, AppResult};

pub fn query_exchange_rate(
    _deps: Deps,
    _denom: impl Into<String>,
    _app: &App,
) -> AppResult<Decimal> {
    // In the first iteration, all deposited tokens are assumed to be equal to 1
    Ok(Decimal::one())
}

// Returns a hashmap with all request exchange rates
pub fn query_all_exchange_rates(
    deps: Deps,
    denoms: impl Iterator<Item = String>,
    app: &App,
) -> AppResult<HashMap<String, Decimal>> {
    denoms
        .into_iter()
        .map(|denom| Ok((denom.clone(), query_exchange_rate(deps, denom, app)?)))
        .collect()
}
