use std::collections::HashMap;

use cosmwasm_std::{coin, Coin, Coins, Decimal, Deps, Uint128};

use crate::{
    contract::{App, AppResult},
    exchange_rate::query_all_exchange_rates,
    helpers::{compute_total_value, compute_value},
    state::STRATEGY_CONFIG,
    yield_sources::{yield_type::YieldType, AssetShare, Strategy, StrategyElement},
};

use cosmwasm_schema::cw_serde;

use crate::{error::AppError, msg::InternalExecuteMsg};

pub fn generate_deposit_strategy(
    deps: Deps,
    funds: Vec<Coin>,
    yield_source_params: Option<Vec<Option<Vec<AssetShare>>>>,
    app: &App,
) -> AppResult<(Vec<(StrategyElement, Decimal)>, Vec<InternalExecuteMsg>)> {
    // This is the storage strategy for all assets
    let target_strategy = STRATEGY_CONFIG.load(deps.storage)?;

    // This is the current distribution of funds inside the strategies
    let current_strategy_status = target_strategy.query_current_status(deps, app)?;

    let mut usable_funds: Coins = funds.try_into()?;
    let (withdraw_strategy, mut this_deposit_strategy) = target_strategy.current_deposit_strategy(
        deps,
        &mut usable_funds,
        current_strategy_status,
        app,
    )?;

    // We query the yield source shares
    this_deposit_strategy.apply_current_strategy_shares(deps, app)?;

    // We correct it if the user asked to correct the share parameters of each strategy
    this_deposit_strategy.correct_with(yield_source_params);

    // We fill the strategies with the current deposited funds and get messages to execute those deposits
    let deposit_msgs =
        this_deposit_strategy.fill_all_and_get_messages(deps, usable_funds.into(), app)?;

    Ok((withdraw_strategy, deposit_msgs))
}

impl Strategy {
    // We determine the best balance strategy depending on the current deposits and the target strategy.
    // This method needs to be called on the stored strategy
    // We error if deposit value is non-zero here
    pub fn current_deposit_strategy(
        &self,
        deps: Deps,
        funds: &mut Coins,
        current_strategy_status: Self,
        app: &App,
    ) -> AppResult<(Vec<(StrategyElement, Decimal)>, Self)> {
        let total_value = self.current_balance(deps, app)?.total_value;
        let deposit_value = compute_value(deps, &funds.to_vec(), app)?;

        if (total_value + deposit_value).is_zero() {
            return Err(AppError::NoDeposit {});
        }

        // We create the strategy so that he final distribution is as close to the target strategy as possible
        // 1. For all strategies, we withdraw some if its value is too high above target_strategy
        let mut withdraw_value = Uint128::zero();

        // All strategies have to be reviewed
        // EITHER of those are true :
        // - The yield source has too much funds deposited and some should be withdrawn
        // OR
        // - Some funds need to be deposited into the strategy

        // First we generate the messages for withdrawing strategies that have too much funds
        let withdraw_strategy: Vec<(StrategyElement, Decimal)> = current_strategy_status
            .0
            .iter()
            .zip(self.0.clone())
            .map(|(target, current)| {
                // We need to take into account the total value added by the current shares
                let value_now = current.share * total_value;
                let target_value = target.share * (total_value + deposit_value);

                // If value now is greater than the target value, we need to withdraw some funds from the protocol
                if target_value < value_now {
                    let this_withdraw_value = value_now - target_value;
                    // In the following line, total_value can't be zero, otherwise the if condition wouldn't be met
                    let this_withdraw_share = Decimal::from_ratio(this_withdraw_value, total_value);
                    let this_withdraw_funds =
                        current.withdraw_preview(deps, Some(this_withdraw_share), app)?;
                    withdraw_value += this_withdraw_value;
                    for fund in this_withdraw_funds {
                        funds.add(fund)?;
                    }

                    // In case there is a withdraw from the strategy, we don't need to deposit into this strategy after !
                    Ok::<_, AppError>(Some((current, this_withdraw_share)))
                } else {
                    Ok(None)
                }
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        let available_value = withdraw_value + deposit_value;

        let this_deposit_strategy: Strategy = current_strategy_status
            .0
            .into_iter()
            .zip(self.0.clone())
            .flat_map(|(target, current)| {
                // We need to take into account the total value added by the current shares
                let value_now = current.share * total_value;
                let target_value = target.share * (total_value + deposit_value);

                // If value now is smaller than the target value, we need to deposit some funds into the protocol
                if target_value < value_now {
                    None
                } else {
                    // In case we don't withdraw anything, it means we might deposit.
                    let share = if available_value.is_zero() {
                        Decimal::zero()
                    } else {
                        Decimal::from_ratio(target_value - value_now, available_value)
                    };

                    Some(StrategyElement {
                        yield_source: target.yield_source.clone(),
                        share,
                    })
                }
            })
            .collect::<Vec<_>>()
            .into();

        //     // Then we create the deposit elements to generate the deposits
        //
        // })
        // .collect::<Result<Vec<_>, _>>()?
        // .into_iter()
        // .flatten()
        // .collect::<Vec<_>>()
        // .into();

        Ok((withdraw_strategy, this_deposit_strategy))
    }

    // We dispatch the available funds directly into the Strategies
    // This returns :
    // 0 : Funds that are used for specific strategies. And remaining amounts to fill those strategies
    // 1 : Funds that are still available to fill those strategies
    // This is the algorithm that is implemented here
    fn fill_sources(
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
                    .map(|AssetShare { denom, share }| {
                        // Amount to fill this denom completely is value / exchange_rate
                        // Value we want to put here is share * source.share * total_value
                        Ok::<_, AppError>(StrategyStatusElement {
                            denom: denom.clone(),
                            raw_funds: Uint128::zero(),
                            remaining_amount: (share * source.share
                                / exchange_rates
                                    .get(denom)
                                    .ok_or(AppError::NoExchangeRate(denom.clone()))?)
                                * total_value,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;

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
                    .find(|(AssetShare { denom, share: _ }, _status)| this_coin.denom.eq(denom))
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

    fn fill_all(
        &self,
        deps: Deps,
        funds: Vec<Coin>,
        app: &App,
    ) -> AppResult<Vec<OneDepositStrategy>> {
        // We determine the value of all tokens that will be used inside this function
        let exchange_rates = query_all_exchange_rates(
            deps,
            funds
                .iter()
                .map(|f| f.denom.clone())
                .chain(self.all_denoms()),
            app,
        )?;
        let (status, remaining_funds) = self.fill_sources(funds, &exchange_rates)?;
        status.fill_with_remaining_funds(remaining_funds, &exchange_rates)
    }

    /// Gets the deposit messages from a given strategy by filling all strategies with the associated funds
    pub fn fill_all_and_get_messages(
        &self,
        deps: Deps,
        funds: Vec<Coin>,
        app: &App,
    ) -> AppResult<Vec<InternalExecuteMsg>> {
        let deposit_strategies = self.fill_all(deps, funds, app)?;
        Ok(deposit_strategies
            .iter()
            .zip(self.0.iter().map(|s| s.yield_source.ty.clone()))
            .enumerate()
            .map(|(index, (strategy, yield_type))| strategy.deposit_msgs(index, yield_type))
            .collect())
    }

    /// Corrects the current strategy with some parameters passed by the user
    pub fn correct_with(&mut self, params: Option<Vec<Option<Vec<AssetShare>>>>) {
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
struct StrategyStatusElement {
    pub denom: String,
    pub raw_funds: Uint128,
    pub remaining_amount: Uint128,
}

/// This contains information about the strategy status
/// AFTER filling with unrelated coins
/// Before filling with related coins
#[cw_serde]
struct StrategyStatus(pub Vec<Vec<StrategyStatusElement>>);

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
    pub fn deposit_msgs(&self, yield_index: usize, yield_type: YieldType) -> InternalExecuteMsg {
        // For each strategy, we send a message on the contract to execute it
        InternalExecuteMsg::DepositOneStrategy {
            swap_strategy: self.clone(),
            yield_type,
            yield_index,
        }
    }
}
