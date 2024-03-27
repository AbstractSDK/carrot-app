use crate::contract::{App, AppResult};
use abstract_app::traits::AccountIdentification;
use abstract_app::{objects::AssetEntry, traits::AbstractNameService};
use abstract_sdk::Resolve;
use cosmwasm_std::{Addr, Coin, Coins, Decimal, Deps, StdResult, Uint128};

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

pub const CLOSE_PER_MILLE: u64 = 1;

/// Returns wether actual is close to expected within CLOSE_PER_MILLE per mille
pub fn close_to(expected: Decimal, actual: Decimal) -> bool {
    let close_coeff = expected * Decimal::permille(CLOSE_PER_MILLE);

    if expected == Decimal::zero() {
        return actual < close_coeff;
    }

    actual > expected * (Decimal::one() - close_coeff)
        && actual < expected * (Decimal::one() + close_coeff)
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use cosmwasm_std::Decimal;

    use crate::helpers::close_to;

    #[test]
    fn not_close_to() {
        assert!(!close_to(Decimal::percent(99), Decimal::one()))
    }

    #[test]
    fn actually_close_to() {
        assert!(close_to(
            Decimal::from_str("0.99999").unwrap(),
            Decimal::one()
        ));
    }
}
