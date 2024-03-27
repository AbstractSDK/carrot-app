use crate::contract::{App, AppResult};
use crate::error::AppError;
use abstract_app::traits::AccountIdentification;
use abstract_app::{
    objects::{AnsAsset, AssetEntry},
    traits::AbstractNameService,
};
use abstract_money_market_adapter::msg::MoneyMarketQueryMsg;
use abstract_money_market_adapter::MoneyMarketInterface;
use cosmwasm_std::{ensure_eq, Coin, CosmosMsg, Decimal, Deps, SubMsg, Uint128};
use cw_asset::AssetInfo;

use abstract_money_market_standard::query::MoneyMarketAnsQuery;

pub const MARS_MONEY_MARKET: &str = "mars";

pub fn deposit(deps: Deps, denom: String, amount: Uint128, app: &App) -> AppResult<Vec<SubMsg>> {
    let ans = app.name_service(deps);
    let ans_fund = ans.query(&AssetInfo::native(denom))?;

    Ok(vec![SubMsg::new(
        app.ans_money_market(deps, MARS_MONEY_MARKET.to_string())
            .deposit(AnsAsset::new(ans_fund, amount))?,
    )])
}

pub fn withdraw(
    deps: Deps,
    denom: String,
    amount: Option<Uint128>,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
    let ans = app.name_service(deps);

    let amount = if let Some(amount) = amount {
        amount
    } else {
        user_deposit(deps, denom.clone(), &app)?
    };

    let ans_fund = ans.query(&AssetInfo::native(denom))?;

    Ok(vec![app
        .ans_money_market(deps, MARS_MONEY_MARKET.to_string())
        .withdraw(AnsAsset::new(ans_fund, amount))?
        .into()])
}

pub fn withdraw_rewards(
    deps: Deps,
    denom: String,
    app: &App,
) -> AppResult<(Vec<Coin>, Vec<CosmosMsg>)> {
    // Mars doesn't have rewards, it's automatically auto-compounded
    Ok((vec![], vec![]))
}

pub fn user_deposit(deps: Deps, denom: String, app: &App) -> AppResult<Uint128> {
    let ans = app.name_service(deps);
    let asset = ans.query(&AssetInfo::native(denom))?;
    let user = app.account_base(deps)?.proxy;

    Ok(app
        .ans_money_market(deps, MARS_MONEY_MARKET.to_string())
        .query(MoneyMarketQueryMsg::MoneyMarketAnsQuery {
            query: MoneyMarketAnsQuery::UserDeposit {
                user: user.to_string(),
                asset,
            },
            money_market: MARS_MONEY_MARKET.to_string(),
        })?)
}

/// Returns an amount representing a user's liquidity
pub fn user_liquidity(deps: Deps, denom: String, app: &App) -> AppResult<Uint128> {
    user_deposit(deps, denom, app)
}

pub fn user_rewards(deps: Deps, denom: String, app: &App) -> AppResult<Vec<Coin>> {
    // No rewards, because mars is self-auto-compounding

    Ok(vec![])
}
