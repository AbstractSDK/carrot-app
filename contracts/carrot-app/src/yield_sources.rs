pub mod mars;
pub mod osmosis_cl_pool;
pub mod yield_type;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    coin, ensure, ensure_eq, wasm_execute, Coin, Coins, CosmosMsg, Decimal, Deps, Env, Uint128,
};
use cw_asset::AssetInfo;
use std::collections::HashMap;

use crate::{
    contract::{App, AppResult},
    error::AppError,
    helpers::close_to,
    msg::{AppExecuteMsg, ExecuteMsg, InternalExecuteMsg},
    state::compute_total_value,
};
use abstract_app::traits::AbstractNameService;

use self::yield_type::YieldType;

/// A yield sources has the following elements
/// A vector of tokens that NEED to be deposited inside the yield source with a repartition of tokens
/// A type that allows routing to the right smart-contract integration internally
#[cw_serde]
pub struct YieldSource {
    /// This id (denom, share)
    pub asset_distribution: Vec<ExpectedToken>,
    pub ty: YieldType,
}

impl YieldSource {
    pub fn check(&self, deps: Deps, app: &App) -> AppResult<()> {
        // First we check the share sums the 100
        let share_sum: Decimal = self.asset_distribution.iter().map(|e| e.share).sum();
        ensure!(
            close_to(Decimal::one(), share_sum),
            AppError::InvalidStrategySum { share_sum }
        );
        ensure!(
            !self.asset_distribution.is_empty(),
            AppError::InvalidEmptyStrategy {}
        );

        // We ensure all deposited tokens exist in ANS
        let ans = app.name_service(deps);
        ans.host().query_assets_reverse(
            &deps.querier,
            &self
                .asset_distribution
                .iter()
                .map(|e| AssetInfo::native(e.denom.clone()))
                .collect::<Vec<_>>(),
        )?;

        // Then we check every yield strategy underneath
        match &self.ty {
            YieldType::ConcentratedLiquidityPool(params) => {
                // A valid CL pool strategy is for 2 assets
                ensure_eq!(
                    self.asset_distribution.len(),
                    2,
                    AppError::InvalidStrategy {}
                );
                params.check(deps)?;
            }
            YieldType::Mars(denom) => {
                // We verify there is only one element in the shares vector
                ensure_eq!(
                    self.asset_distribution.len(),
                    1,
                    AppError::InvalidStrategy {}
                );
                // We verify the first element correspond to the mars deposit denom
                ensure_eq!(
                    &self.asset_distribution[0].denom,
                    denom,
                    AppError::InvalidStrategy {}
                );
            }
        }

        Ok(())
    }
}

#[cw_serde]
pub struct ExpectedToken {
    pub denom: String,
    pub share: Decimal,
}

#[cw_serde]
pub enum ShareType {
    /// This allows using the current distribution of tokens inside the position to compute the distribution on deposit
    Dynamic,
    /// This forces the position to use the target distribution of tokens when depositing
    Fixed,
}

// Related to balance strategies
#[cw_serde]
pub struct BalanceStrategy(pub Vec<BalanceStrategyElement>);

#[cw_serde]
pub struct BalanceStrategyElement {
    pub yield_source: YieldSource,
    pub share: Decimal,
}
impl BalanceStrategyElement {
    pub fn check(&self, deps: Deps, app: &App) -> AppResult<()> {
        self.yield_source.check(deps, app)
    }
}

impl BalanceStrategy {
    pub fn check(&self, deps: Deps, app: &App) -> AppResult<()> {
        // First we check the share sums the 100
        let share_sum: Decimal = self.0.iter().map(|e| e.share).sum();
        ensure!(
            close_to(Decimal::one(), share_sum),
            AppError::InvalidStrategySum { share_sum }
        );
        ensure!(!self.0.is_empty(), AppError::InvalidEmptyStrategy {});

        // Then we check every yield strategy underneath
        for yield_source in &self.0 {
            yield_source.check(deps, app)?;
        }

        Ok(())
    }

    // We dispatch the available funds directly into the Strategies
    // This returns :
    // 0 : Funds that are used for specific strategies. And remaining amounts to fill those strategies
    // 1 : Funds that are still available to fill those strategies
    // This is the algorithm that is implemented here
    pub fn fill_sources(
        &self,
        funds: Vec<Coin>,
        exchange_rates: &HashMap<String, Decimal>,
    ) -> AppResult<(StrategyStatus, Coins)> {
        let total_value = compute_total_value(&funds, exchange_rates)?;
        let mut remaining_funds = Coins::default();

        // We create the vector that holds the funds information
        let mut yield_source_status = self
            .0
            .iter()
            .map(|source| {
                source
                    .yield_source
                    .asset_distribution
                    .iter()
                    .map(|ExpectedToken { denom, share }| B {
                        denom: denom.clone(),
                        raw_funds: Uint128::zero(),
                        remaining_amount: share * source.share * total_value,
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        for this_coin in funds {
            let mut remaining_amount = this_coin.amount;
            // We distribute those funds in to the accepting strategies
            for (strategy, status) in self.0.iter().zip(yield_source_status.iter_mut()) {
                // Find the share for the specific denom inside the strategy
                let this_denom_status = strategy
                    .yield_source
                    .asset_distribution
                    .iter()
                    .zip(status.iter_mut())
                    .find(|(ExpectedToken { denom, share: _ }, _status)| this_coin.denom.eq(denom))
                    .map(|(_, status)| status);

                if let Some(status) = this_denom_status {
                    // We fill the needed value with the remaining_amount
                    let funds_to_use_here = remaining_amount.min(status.remaining_amount);

                    // Those funds are not available for other yield sources
                    remaining_amount -= funds_to_use_here;

                    status.raw_funds += funds_to_use_here;
                    status.remaining_amount -= funds_to_use_here;
                }
            }
            remaining_funds.add(coin(remaining_amount.into(), this_coin.denom))?;
        }

        Ok((yield_source_status.into(), remaining_funds))
    }

    pub fn fill_all(
        &self,
        funds: Vec<Coin>,
        exchange_rates: &HashMap<String, Decimal>,
    ) -> AppResult<Vec<OneDepositStrategy>> {
        let (status, remaining_funds) = self.fill_sources(funds, exchange_rates)?;
        status.fill_with_remaining_funds(remaining_funds, exchange_rates)
    }
}

#[cw_serde]
pub struct B {
    pub denom: String,
    pub raw_funds: Uint128,
    pub remaining_amount: Uint128,
}

/// This contains information about the strategy status
/// AFTER filling with unrelated coins
/// Before filling with related coins
#[cw_serde]
pub struct StrategyStatus(pub Vec<Vec<B>>);

impl From<Vec<Vec<B>>> for StrategyStatus {
    fn from(value: Vec<Vec<B>>) -> Self {
        Self(value)
    }
}

impl StrategyStatus {
    pub fn fill_with_remaining_funds(
        &self,
        mut funds: Coins,
        exchange_rates: &HashMap<String, Decimal>,
    ) -> AppResult<Vec<OneDepositStrategy>> {
        self.0
            .iter()
            .map(|f| {
                f.clone()
                    .iter_mut()
                    .map(|status| {
                        let mut swaps = vec![];
                        for fund in funds.to_vec() {
                            let direct_e_r = exchange_rates
                                .get(&fund.denom)
                                .ok_or(AppError::NoExchangeRate(fund.denom.clone()))?
                                / exchange_rates
                                    .get(&status.denom)
                                    .ok_or(AppError::NoExchangeRate(status.denom.clone()))?;
                            let available_coin_in_destination_amount = fund.amount * direct_e_r;

                            let fill_amount =
                                available_coin_in_destination_amount.min(status.remaining_amount);

                            let swap_in_amount = fill_amount * (Decimal::one() / direct_e_r);

                            if swap_in_amount != Uint128::zero() {
                                status.remaining_amount -= fill_amount;
                                let swap_funds = coin(swap_in_amount.into(), fund.denom);
                                funds.sub(swap_funds.clone())?;
                                swaps.push(DepositStep::Swap {
                                    asset_in: swap_funds,
                                    denom_out: status.denom.clone(),
                                    expected_amount: fill_amount,
                                });
                            }
                        }
                        if !status.raw_funds.is_zero() {
                            swaps.push(DepositStep::UseFunds {
                                asset: coin(status.raw_funds.into(), status.denom.clone()),
                            })
                        }

                        Ok::<_, AppError>(swaps)
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(Into::into)
            })
            .collect::<Result<Vec<OneDepositStrategy>, _>>()
    }
}

#[cw_serde]
pub enum DepositStep {
    Swap {
        asset_in: Coin,
        denom_out: String,
        expected_amount: Uint128,
    },
    UseFunds {
        asset: Coin,
    },
}

#[cw_serde]
pub struct OneDepositStrategy(pub Vec<Vec<DepositStep>>);

impl From<Vec<Vec<DepositStep>>> for OneDepositStrategy {
    fn from(value: Vec<Vec<DepositStep>>) -> Self {
        Self(value)
    }
}

impl OneDepositStrategy {
    pub fn deposit_msgs(
        &self,
        env: &Env,
        yield_index: usize,
        yield_type: YieldType,
    ) -> AppResult<CosmosMsg> {
        // For each strategy, we send a message on the contract to execute it
        Ok(wasm_execute(
            env.contract.address.clone(),
            &ExecuteMsg::Module(AppExecuteMsg::Internal(
                InternalExecuteMsg::DepositOneStrategy {
                    swap_strategy: self.clone(),
                    yield_type,
                    yield_index,
                },
            )),
            vec![],
        )?
        .into())
    }
}

#[cw_serde]
pub enum DepositStepResult {
    Todo(DepositStep),
    Done { amount: Coin },
}
