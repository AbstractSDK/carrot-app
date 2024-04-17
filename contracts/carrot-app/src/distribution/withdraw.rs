use abstract_app::objects::AnsAsset;
use abstract_sdk::{AccountAction, Execution, ExecutorMsg};
use cosmwasm_std::{Decimal, Deps};

use crate::{
    ans_assets::AnsAssets,
    contract::{App, AppResult},
    error::AppError,
    yield_sources::{yield_type::YieldTypeImplementation, Strategy, StrategyElement},
};

impl Strategy {
    pub fn withdraw(
        self,
        deps: Deps,
        withdraw_share: Option<Decimal>,
        app: &App,
    ) -> AppResult<Vec<ExecutorMsg>> {
        self.0
            .into_iter()
            .map(|s| s.withdraw(deps, withdraw_share, app))
            .collect()
    }
}

impl StrategyElement {
    pub fn withdraw(
        self,
        deps: Deps,
        withdraw_share: Option<Decimal>,
        app: &App,
    ) -> AppResult<ExecutorMsg> {
        let this_withdraw_amount = withdraw_share
            .map(|share| {
                let this_amount = self.yield_source.params.user_liquidity(deps, app)?;
                let this_withdraw_amount = share * this_amount;

                Ok::<_, AppError>(this_withdraw_amount)
            })
            .transpose()?;
        let raw_msg = self
            .yield_source
            .params
            .withdraw(deps, this_withdraw_amount, app)?;

        Ok::<_, AppError>(
            app.executor(deps)
                .execute(vec![AccountAction::from_vec(raw_msg)])?,
        )
    }

    pub fn withdraw_preview(
        &self,
        deps: Deps,
        withdraw_share: Option<Decimal>,
        app: &App,
    ) -> AppResult<AnsAssets> {
        let current_deposit = self.yield_source.params.user_deposit(deps, app)?;

        if let Some(share) = withdraw_share {
            Ok(current_deposit
                .into_iter()
                .map(|funds| AnsAsset::new(funds.name, funds.amount * share))
                .collect::<Vec<_>>()
                .try_into()?)
        } else {
            Ok(current_deposit)
        }
    }
}
