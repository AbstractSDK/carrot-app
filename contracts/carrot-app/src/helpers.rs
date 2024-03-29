use std::collections::HashMap;

use crate::contract::{App, AppResult};
use crate::error::AppError;
use crate::exchange_rate::query_exchange_rate;
use abstract_app::traits::AccountIdentification;
use abstract_app::{objects::AssetEntry, traits::AbstractNameService};
use abstract_sdk::Resolve;
use cosmwasm_std::{Addr, Coin, Coins, Decimal, Deps, Env, MessageInfo, StdResult, Uint128};

pub fn assert_contract(info: &MessageInfo, env: &Env) -> AppResult<()> {
    if info.sender == env.contract.address {
        Ok(())
    } else {
        Err(AppError::Unauthorized {})
    }
}

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

pub fn compute_total_value(
    funds: &[Coin],
    exchange_rates: &HashMap<String, Decimal>,
) -> AppResult<Uint128> {
    funds
        .iter()
        .map(|c| {
            let exchange_rate = exchange_rates
                .get(&c.denom)
                .ok_or(AppError::NoExchangeRate(c.denom.clone()))?;
            Ok(c.amount * *exchange_rate)
        })
        .sum()
}

pub fn compute_value(deps: Deps, funds: &[Coin], app: &App) -> AppResult<Uint128> {
    funds
        .iter()
        .map(|c| {
            let exchange_rate = query_exchange_rate(deps, c.denom.clone(), app)?;
            Ok(c.amount * exchange_rate)
        })
        .sum()
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
