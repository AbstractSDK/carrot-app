use cosmwasm_std::{Decimal, Deps, Uint128};

use crate::{
    ans_assets::AnsAssets,
    contract::{App, AppResult},
    error::AppError,
    exchange_rate::query_exchange_rate,
    msg::AssetsBalanceResponse,
    yield_sources::{
        yield_type::YieldTypeImplementation, AssetShare, Strategy, StrategyElement, YieldSource,
    },
};
impl Strategy {
    // Returns the total balance
    pub fn current_balance(&self, deps: Deps, app: &App) -> AppResult<AssetsBalanceResponse> {
        let mut funds = AnsAssets::default();
        let mut total_value = Uint128::zero();
        self.0.iter().try_for_each(|s| {
            let deposit_value = s
                .yield_source
                .params
                .user_deposit(deps, app)
                .unwrap_or_default();
            for fund in deposit_value {
                let exchange_rate = query_exchange_rate(deps, &fund.name, app)?;
                funds.add(fund.clone())?;
                total_value += fund.amount * exchange_rate;
            }
            Ok::<_, AppError>(())
        })?;

        Ok(AssetsBalanceResponse {
            balances: funds.into(),
            total_value,
        })
    }

    /// Returns the current status of the full strategy. It returns shares reflecting the underlying positions
    pub fn query_current_status(&self, deps: Deps, app: &App) -> AppResult<Strategy> {
        let all_strategy_values = self
            .0
            .iter()
            .map(|s| s.query_current_value(deps, app))
            .collect::<Result<Vec<_>, _>>()?;

        let all_strategies_value: Uint128 =
            all_strategy_values.iter().map(|(value, _)| value).sum();

        // If there is no value, the current status is the stored strategy
        if all_strategies_value.is_zero() {
            return Ok(self.clone());
        }

        // Finally, we dispatch the total_value to get investment shares
        Ok(self
            .0
            .iter()
            .zip(all_strategy_values)
            .map(|(original_strategy, (value, shares))| StrategyElement {
                yield_source: YieldSource {
                    asset_distribution: shares,
                    params: original_strategy.yield_source.params.clone(),
                },
                share: Decimal::from_ratio(value, all_strategies_value),
            })
            .collect::<Vec<_>>()
            .into())
    }

    /// This function applies the underlying shares inside yield sources to each yield source depending on the current strategy state
    pub fn apply_current_strategy_shares(&mut self, deps: Deps, app: &App) -> AppResult<()> {
        self.0.iter_mut().try_for_each(|yield_source| {
            match yield_source.yield_source.params.share_type() {
                crate::yield_sources::ShareType::Dynamic => {
                    let (_total_value, shares) = yield_source.query_current_value(deps, app)?;
                    yield_source.yield_source.asset_distribution = shares;
                }
                crate::yield_sources::ShareType::Fixed => {}
            };

            Ok::<_, AppError>(())
        })?;
        Ok(())
    }
}

impl StrategyElement {
    /// Queries the current value distribution of a registered strategy
    /// If there is no deposit or the query for the user deposit value fails
    ///     the function returns 0 value with the registered asset distribution
    pub fn query_current_value(
        &self,
        deps: Deps,
        app: &App,
    ) -> AppResult<(Uint128, Vec<AssetShare>)> {
        // If there is no deposit
        let user_deposit = match self.yield_source.params.user_deposit(deps, app) {
            Ok(deposit) => deposit,
            Err(_) => {
                return Ok((
                    Uint128::zero(),
                    self.yield_source.asset_distribution.clone(),
                ))
            }
        };

        // From this, we compute the shares within the investment
        let each_value = user_deposit
            .iter()
            .map(|fund| {
                let exchange_rate = query_exchange_rate(deps, &fund.name, app)?;

                Ok::<_, AppError>((fund.name.clone(), exchange_rate * fund.amount))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let total_value: Uint128 = each_value.iter().map(|(_denom, amount)| amount).sum();

        // If there is no value, the current status is the stored strategy
        if total_value.is_zero() {
            return Ok((
                Uint128::zero(),
                self.yield_source.asset_distribution.clone(),
            ));
        }

        let each_shares = each_value
            .into_iter()
            .map(|(asset, amount)| {
                Ok::<_, AppError>(AssetShare {
                    asset,
                    share: Decimal::from_ratio(amount, total_value),
                })
            })
            .collect::<Result<_, _>>()?;
        Ok((total_value, each_shares))
    }
}
