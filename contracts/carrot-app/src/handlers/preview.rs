use cosmwasm_std::{Coin, Decimal, Deps, Uint128};

use crate::{
    contract::{App, AppResult},
    distribution::deposit::generate_deposit_strategy,
    msg::{DepositPreviewResponse, UpdateStrategyPreviewResponse, WithdrawPreviewResponse},
    state::STRATEGY_CONFIG,
    yield_sources::{AssetShare, StrategyUnchecked},
};

use super::query::withdraw_share;

pub fn deposit_preview(
    deps: Deps,
    funds: Vec<Coin>,
    yield_source_params: Option<Vec<Option<Vec<AssetShare>>>>,
    app: &App,
) -> AppResult<DepositPreviewResponse> {
    let (withdraw_strategy, deposit_strategy) =
        generate_deposit_strategy(deps, funds, yield_source_params, app)?;

    Ok(DepositPreviewResponse {
        withdraw: withdraw_strategy
            .into_iter()
            .map(|(el, share)| (el.into(), share))
            .collect(),
        deposit: deposit_strategy,
    })
}

pub fn withdraw_preview(
    deps: Deps,
    amount: Option<Uint128>,
    app: &App,
) -> AppResult<WithdrawPreviewResponse> {
    let withdraw_share = withdraw_share(deps, amount, app)?;
    let funds = STRATEGY_CONFIG
        .load(deps.storage)?
        .withdraw_preview(deps, withdraw_share, app)?;

    let msgs = STRATEGY_CONFIG
        .load(deps.storage)?
        .withdraw(deps, withdraw_share, app)?;

    Ok(WithdrawPreviewResponse {
        share: withdraw_share.unwrap_or(Decimal::one()),
        funds,
        msgs: msgs.into_iter().map(Into::into).collect(),
    })
}

pub fn update_strategy_preview(
    deps: Deps,
    funds: Vec<Coin>,
    strategy: StrategyUnchecked,
    app: &App,
) -> AppResult<UpdateStrategyPreviewResponse> {
    Ok(UpdateStrategyPreviewResponse {})
}
