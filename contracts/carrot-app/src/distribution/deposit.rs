use std::collections::HashMap;

use cosmwasm_std::{coin, Coin, Coins, Decimal, Uint128};

use crate::{
    contract::AppResult,
    helpers::compute_total_value,
    yield_sources::{yield_type::YieldType, BalanceStrategy, ExpectedToken},
};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{wasm_execute, CosmosMsg, Env};

use crate::{
    error::AppError,
    msg::{AppExecuteMsg, ExecuteMsg, InternalExecuteMsg},
};

impl BalanceStrategy {
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
                    .map(|ExpectedToken { denom, share }| StrategyStatusElement {
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

    /// Gets the deposit messages from a given strategy by filling all strategies with the associated funds
    pub fn fill_all_and_get_messages(
        &self,
        env: &Env,
        funds: Vec<Coin>,
        exchange_rates: &HashMap<String, Decimal>,
    ) -> AppResult<Vec<CosmosMsg>> {
        let deposit_strategies = self.fill_all(funds, exchange_rates)?;
        deposit_strategies
            .iter()
            .zip(self.0.iter().map(|s| s.yield_source.ty.clone()))
            .enumerate()
            .map(|(index, (strategy, yield_type))| strategy.deposit_msgs(env, index, yield_type))
            .collect()
    }

    /// Corrects the current strategy with some parameters passed by the user
    pub fn correct_with(&mut self, params: Option<Vec<Option<Vec<ExpectedToken>>>>) {
        // We correct the strategy if specified in parameters
        let params = params.unwrap_or_else(|| vec![None; self.0.len()]);

        self.0
            .iter_mut()
            .zip(params)
            .for_each(|(strategy, params)| {
                if let Some(param) = params {
                    strategy.yield_source.asset_distribution = param;
                }
            })
    }
}

#[cw_serde]
pub struct StrategyStatusElement {
    pub denom: String,
    pub raw_funds: Uint128,
    pub remaining_amount: Uint128,
}

/// This contains information about the strategy status
/// AFTER filling with unrelated coins
/// Before filling with related coins
#[cw_serde]
pub struct StrategyStatus(pub Vec<Vec<StrategyStatusElement>>);

impl From<Vec<Vec<StrategyStatusElement>>> for StrategyStatus {
    fn from(value: Vec<Vec<StrategyStatusElement>>) -> Self {
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
