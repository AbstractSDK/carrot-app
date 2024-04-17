use std::collections::HashMap;

use abstract_app::objects::AssetEntry;
use cosmwasm_std::{Decimal, Deps};

use crate::contract::{App, AppResult};

pub fn query_exchange_rate(_deps: Deps, _name: &AssetEntry, _app: &App) -> AppResult<Decimal> {
    // In the first iteration, all deposited tokens are assumed to be equal to 1
    Ok(Decimal::one())
}

// Returns a hashmap with all request exchange rates
pub fn query_all_exchange_rates(
    deps: Deps,
    assets: impl Iterator<Item = AssetEntry>,
    app: &App,
) -> AppResult<HashMap<String, Decimal>> {
    assets
        .into_iter()
        .map(|asset| Ok((asset.to_string(), query_exchange_rate(deps, &asset, app)?)))
        .collect()
}
