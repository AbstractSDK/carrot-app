use std::collections::HashMap;

use crate::ans_assets::AnsAssets;
use crate::contract::{App, AppResult};
use crate::error::AppError;
use crate::exchange_rate::query_exchange_rate;
use abstract_app::objects::AnsAsset;
use abstract_app::traits::AccountIdentification;
use abstract_app::{objects::AssetEntry, traits::AbstractNameService};
use abstract_sdk::{AbstractSdkResult, Resolve};
use cosmwasm_std::{Addr, Coins, Decimal, Deps, Env, MessageInfo, StdResult, Uint128};
use cw_asset::{Asset, AssetInfo};

pub fn unwrap_native(asset: &AssetInfo) -> AppResult<String> {
    match asset {
        cw_asset::AssetInfoBase::Native(denom) => Ok(denom.clone()),
        cw_asset::AssetInfoBase::Cw20(_) => Err(AppError::NonNativeAsset {}),
        _ => Err(AppError::NonNativeAsset {}),
    }
}

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

pub fn get_proxy_balance(deps: Deps, asset: &AssetEntry, app: &App) -> AppResult<Uint128> {
    let fund = app.name_service(deps).query(asset)?;
    Ok(fund.query_balance(&deps.querier, app.account_base(deps)?.proxy)?)
}

pub fn add_funds(assets: Vec<AnsAsset>, to_add: AnsAsset) -> StdResult<Vec<AnsAsset>> {
    let mut assets: AnsAssets = assets.try_into()?;
    assets.add(to_add)?;
    Ok(assets.into())
}

pub const CLOSE_COEFF: Decimal = Decimal::permille(1);

/// Returns wether actual is close to expected within CLOSE_PER_MILLE per mille
pub fn close_to(expected: Decimal, actual: Decimal) -> bool {
    if expected == Decimal::zero() {
        return actual < CLOSE_COEFF;
    }

    actual > expected * (Decimal::one() - CLOSE_COEFF)
        && actual < expected * (Decimal::one() + CLOSE_COEFF)
}

pub fn compute_total_value(
    funds: &[AnsAsset],
    exchange_rates: &HashMap<String, Decimal>,
) -> AppResult<Uint128> {
    funds
        .iter()
        .map(|c| {
            let exchange_rate = exchange_rates
                .get(&c.name.to_string())
                .ok_or(AppError::NoExchangeRate(c.name.clone()))?;
            Ok(c.amount * *exchange_rate)
        })
        .sum()
}

pub fn compute_value(deps: Deps, funds: &[AnsAsset], app: &App) -> AppResult<Uint128> {
    funds
        .iter()
        .map(|c| {
            let exchange_rate = query_exchange_rate(deps, &c.name, app)?;
            Ok(c.amount * exchange_rate)
        })
        .sum()
}

pub fn to_ans_assets(deps: Deps, funds: Coins, app: &App) -> AbstractSdkResult<Vec<AnsAsset>> {
    let ans = app.name_service(deps);
    funds
        .into_iter()
        .map(|fund| ans.query(&Asset::from(fund)))
        .collect()
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
