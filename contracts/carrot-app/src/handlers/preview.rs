use abstract_app::objects::AnsAsset;
use cosmwasm_std::{Decimal, Deps, Uint128};

use crate::{
    ans_assets::AnsAssets,
    check::Checkable,
    contract::{App, AppResult},
    distribution::deposit::generate_deposit_strategy,
    error::AppError,
    msg::{DepositPreviewResponse, UpdateStrategyPreviewResponse, WithdrawPreviewResponse},
    state::STRATEGY_CONFIG,
    yield_sources::{AssetShare, StrategyUnchecked},
};

use super::query::withdraw_share;

pub fn deposit_preview(
    deps: Deps,
    assets: Vec<AnsAsset>,
    yield_source_params: Option<Vec<Option<Vec<AssetShare>>>>,
    app: &App,
) -> AppResult<DepositPreviewResponse> {
    let target_strategy = STRATEGY_CONFIG.load(deps.storage)?;
    let (withdraw_strategy, deposit_strategy) = generate_deposit_strategy(
        deps,
        assets.try_into()?,
        target_strategy,
        yield_source_params,
        app,
    )?;

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
    assets: Vec<AnsAsset>,
    strategy: StrategyUnchecked,
    app: &App,
) -> AppResult<UpdateStrategyPreviewResponse> {
    // We withdraw outstanding strategies

    let old_strategy = STRATEGY_CONFIG.load(deps.storage)?;

    // We check the new strategy
    let strategy = strategy.check(deps, app)?;

    // We execute operations to rebalance the funds between the strategies
    let mut available_assets: AnsAssets = assets.try_into()?;
    // 1. We withdraw all yield_sources that are not included in the new strategies
    let all_stale_sources: Vec<_> = old_strategy
        .0
        .into_iter()
        .filter(|x| !strategy.0.contains(x))
        .collect();

    all_stale_sources.clone().into_iter().try_for_each(|s| {
        let assets = s.withdraw_preview(deps, None, app).unwrap_or_default();
        available_assets.extend(assets)?;
        Ok::<_, AppError>(())
    })?;

    // 3. We deposit the funds into the new strategy
    let (withdraw_strategy, deposit_strategy) =
        generate_deposit_strategy(deps, available_assets, strategy, None, app)?;

    let withdraw_strategy = [
        all_stale_sources
            .into_iter()
            .map(|s| (s, Decimal::one()))
            .collect(),
        withdraw_strategy,
    ]
    .concat();

    Ok(UpdateStrategyPreviewResponse {
        withdraw: withdraw_strategy
            .into_iter()
            .map(|(el, share)| (el.into(), share))
            .collect(),
        deposit: deposit_strategy,
    })
}
