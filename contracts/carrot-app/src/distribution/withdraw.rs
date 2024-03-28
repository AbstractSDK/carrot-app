use abstract_sdk::{AccountAction, Execution, ExecutorMsg};
use cosmwasm_std::{Decimal, Deps};

use crate::{
    contract::{App, AppResult},
    error::AppError,
    yield_sources::BalanceStrategy,
};

impl BalanceStrategy {
    pub fn withdraw(
        self,
        deps: Deps,
        withdraw_share: Option<Decimal>,
        app: &App,
    ) -> AppResult<Vec<ExecutorMsg>> {
        self.0
            .into_iter()
            .map(|s| {
                let this_withdraw_amount = withdraw_share
                    .map(|share| {
                        let this_amount = s.yield_source.ty.user_liquidity(deps, app)?;
                        let this_withdraw_amount = share * this_amount;

                        Ok::<_, AppError>(this_withdraw_amount)
                    })
                    .transpose()?;
                let raw_msg = s
                    .yield_source
                    .ty
                    .withdraw(deps, this_withdraw_amount, app)?;

                Ok::<_, AppError>(
                    app.executor(deps)
                        .execute(vec![AccountAction::from_vec(raw_msg)])?,
                )
            })
            .collect()
    }
}
