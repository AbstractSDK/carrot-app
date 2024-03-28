use abstract_sdk::{AccountAction, Execution, ExecutorMsg};
use cosmwasm_std::{Coin, Decimal, Deps};

use crate::{
    contract::{App, AppResult},
    error::AppError,
    handlers::query::query_all_exchange_rates,
    helpers::compute_total_value,
    msg::AssetsBalanceResponse,
    yield_sources::{BalanceStrategy, BalanceStrategyElement},
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
            .map(|s| s.withdraw(deps, withdraw_share, app))
            .collect()
    }
}

impl BalanceStrategyElement {
    pub fn withdraw(
        self,
        deps: Deps,
        withdraw_share: Option<Decimal>,
        app: &App,
    ) -> AppResult<ExecutorMsg> {
        let this_withdraw_amount = withdraw_share
            .map(|share| {
                let this_amount = self.yield_source.ty.user_liquidity(deps, app)?;
                let this_withdraw_amount = share * this_amount;

                Ok::<_, AppError>(this_withdraw_amount)
            })
            .transpose()?;
        let raw_msg = self
            .yield_source
            .ty
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
    ) -> AppResult<Vec<Coin>> {
        let current_deposit = self.yield_source.ty.user_deposit(deps, app)?;
        let exchange_rates =
            query_all_exchange_rates(deps, current_deposit.iter().map(|f| f.denom.clone()), app)?;
        if let Some(share) = withdraw_share {
            Ok(current_deposit
                .into_iter()
                .map(|funds| Coin {
                    denom: funds.denom,
                    amount: funds.amount * share,
                })
                .collect())
        } else {
            Ok(current_deposit)
        }
    }
}
