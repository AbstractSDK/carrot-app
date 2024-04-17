use abstract_app::objects::AnsAsset;
use cosmwasm_std::{Deps, Uint128};

use crate::{
    contract::{App, AppResult},
    distribution::deposit::generate_deposit_strategy,
    msg::{DepositPreviewResponse, UpdateStrategyPreviewResponse, WithdrawPreviewResponse},
    yield_sources::{AssetShare, StrategyUnchecked},
};

pub fn deposit_preview(
    deps: Deps,
    assets: Vec<AnsAsset>,
    yield_source_params: Option<Vec<Option<Vec<AssetShare>>>>,
    app: &App,
) -> AppResult<DepositPreviewResponse> {
    let (withdraw_strategy, deposit_strategy) =
        generate_deposit_strategy(deps, assets.try_into()?, yield_source_params, app)?;

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
    Ok(WithdrawPreviewResponse {})
}
pub fn update_strategy_preview(
    deps: Deps,
    assets: Vec<AnsAsset>,
    strategy: StrategyUnchecked,
    app: &App,
) -> AppResult<UpdateStrategyPreviewResponse> {
    Ok(UpdateStrategyPreviewResponse {})
}
