use crate::contract::{App, AppResult};
use abstract_app::traits::AccountIdentification;
use abstract_app::{
    objects::{AnsAsset, AssetEntry},
    traits::AbstractNameService,
};
use cosmwasm_std::{Coin, CosmosMsg, Deps, SubMsg, Uint128};
use cw_asset::AssetInfo;

pub fn deposit(deps: Deps, denom: String, amount: Uint128, app: &App) -> AppResult<Vec<SubMsg>> {
    let ans = app.name_service(deps);
    let ans_fund = ans.query(&AssetInfo::native(denom))?;

    // TODO after MM Adapter is merged
    // Ok(vec![app
    //     .ans_money_market(deps)?
    //     .deposit(AnsAsset::new(ans_fund, amount))?
    //     .into()])

    Ok(vec![])
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

    // TODO after MM Adapter is merged
    // Ok(vec![app
    //     .ans_money_market(deps)?
    //     .withdraw(AnsAsset::new(ans_fund, amount))?
    //     .into()])

    Ok(vec![])
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
    let ans_fund = ans.query(&AssetInfo::native(denom))?;
    let user = app.account_base(deps)?.proxy;

    // TODO after MM Adapter is merged
    // Ok(app
    //     .ans_money_market(deps)?
    //     .user_deposit(user, ans_fund)?
    //     .into())
    Ok(Uint128::zero())
}

/// Returns an amount representing a user's liquidity
pub fn user_liquidity(deps: Deps, denom: String, app: &App) -> AppResult<Uint128> {
    user_deposit(deps, denom, app)
}

pub fn user_rewards(deps: Deps, denom: String, app: &App) -> AppResult<Vec<Coin>> {
    // No rewards, because mars is self-auto-compounding

    Ok(vec![])
}
