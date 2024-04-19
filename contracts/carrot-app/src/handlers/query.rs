use cosmwasm_std::{to_json_binary, Binary, Coin, Coins, Decimal, Deps, Env, Uint128};

use crate::autocompound::get_autocompound_status;
use crate::exchange_rate::query_exchange_rate;
use crate::msg::{PositionResponse, PositionsResponse};
use crate::state::STRATEGY_CONFIG;
use crate::yield_sources::yield_type::YieldTypeImplementation;
use crate::{
    contract::{App, AppResult},
    error::AppError,
    msg::{
        AppQueryMsg, AssetsBalanceResponse, AvailableRewardsResponse, CompoundStatusResponse,
        StrategyResponse,
    },
    state::{Config, CONFIG},
};

use super::preview::{deposit_preview, update_strategy_preview, withdraw_preview};

pub fn query_handler(deps: Deps, env: Env, app: &App, msg: AppQueryMsg) -> AppResult<Binary> {
    match msg {
        AppQueryMsg::Balance {} => to_json_binary(&query_balance(deps, app)?),
        AppQueryMsg::AvailableRewards {} => to_json_binary(&query_rewards(deps, app)?),
        AppQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        AppQueryMsg::Strategy {} => to_json_binary(&query_strategy(deps)?),
        AppQueryMsg::CompoundStatus {} => to_json_binary(&query_compound_status(deps, env, app)?),
        AppQueryMsg::StrategyStatus {} => to_json_binary(&query_strategy_status(deps, app)?),
        AppQueryMsg::Positions {} => to_json_binary(&query_positions(deps, app)?),
        AppQueryMsg::DepositPreview {
            funds,
            yield_sources_params,
        } => to_json_binary(&deposit_preview(deps, funds, yield_sources_params, app)?),
        AppQueryMsg::WithdrawPreview { amount } => {
            to_json_binary(&withdraw_preview(deps, amount, app)?)
        }
        AppQueryMsg::UpdateStrategyPreview { strategy, funds } => {
            to_json_binary(&update_strategy_preview(deps, funds, strategy, app)?)
        }
        AppQueryMsg::FundsValue { funds } => to_json_binary(&query_funds_value(deps, funds, app)?),
    }
    .map_err(Into::into)
}

/// Gets the status of the compounding logic of the application
/// Accounts for the user's ability to pay for the gas fees of executing the contract.
fn query_compound_status(deps: Deps, env: Env, app: &App) -> AppResult<CompoundStatusResponse> {
    let config = CONFIG.load(deps.storage)?;
    let status = get_autocompound_status(
        deps.storage,
        &env,
        config.autocompound_config.cooldown_seconds.u64(),
    )?;

    let (all_rewards, _collect_rewards_msgs) = STRATEGY_CONFIG
        .load(deps.storage)?
        .withdraw_rewards(deps, app)?;

    let funds: Vec<Coin> = all_rewards
        .iter()
        .flat_map(|a| {
            let reward_amount = a.amount * config.autocompound_config.rewards.reward_percent;

            Some(Coin::new(reward_amount.into(), a.denom.clone()))
        })
        .collect();

    Ok(CompoundStatusResponse {
        status,
        execution_rewards: query_funds_value(deps, funds, app)?,
    })
}

pub fn query_strategy(deps: Deps) -> AppResult<StrategyResponse> {
    let strategy = STRATEGY_CONFIG.load(deps.storage)?;

    Ok(StrategyResponse {
        strategy: strategy.into(),
    })
}

pub fn query_strategy_status(deps: Deps, app: &App) -> AppResult<StrategyResponse> {
    let strategy = STRATEGY_CONFIG.load(deps.storage)?;

    Ok(StrategyResponse {
        strategy: strategy.query_current_status(deps, app)?.into(),
    })
}

fn query_config(deps: Deps) -> AppResult<Config> {
    Ok(CONFIG.load(deps.storage)?)
}

pub fn query_balance(deps: Deps, app: &App) -> AppResult<AssetsBalanceResponse> {
    let mut funds = Coins::default();
    let mut total_value = Uint128::zero();

    let strategy = STRATEGY_CONFIG.load(deps.storage)?;
    strategy.0.iter().try_for_each(|s| {
        let deposit_value = s
            .yield_source
            .params
            .user_deposit(deps, app)
            .unwrap_or_default();
        for fund in deposit_value {
            let exchange_rate = query_exchange_rate(deps, fund.denom.clone(), app)?;
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

fn query_rewards(deps: Deps, app: &App) -> AppResult<AvailableRewardsResponse> {
    let strategy = STRATEGY_CONFIG.load(deps.storage)?;

    let mut rewards = Coins::default();
    strategy.0.into_iter().try_for_each(|s| {
        let this_rewards = s.yield_source.params.user_rewards(deps, app)?;
        for fund in this_rewards {
            rewards.add(fund)?;
        }
        Ok::<_, AppError>(())
    })?;

    let mut total_value = Uint128::zero();
    for fund in &rewards {
        let exchange_rate = query_exchange_rate(deps, fund.denom.clone(), app)?;
        total_value += fund.amount * exchange_rate;
    }

    Ok(AvailableRewardsResponse {
        available_rewards: query_funds_value(deps, rewards.into(), app)?,
    })
}

pub fn query_positions(deps: Deps, app: &App) -> AppResult<PositionsResponse> {
    Ok(PositionsResponse {
        positions: STRATEGY_CONFIG
            .load(deps.storage)?
            .0
            .into_iter()
            .map(|s| {
                let balance = s.yield_source.params.user_deposit(deps, app)?;
                let liquidity = s.yield_source.params.user_liquidity(deps, app)?;

                let total_value = balance
                    .iter()
                    .map(|fund| {
                        let exchange_rate = query_exchange_rate(deps, fund.denom.clone(), app)?;
                        Ok(fund.amount * exchange_rate)
                    })
                    .sum::<AppResult<Uint128>>()?;

                Ok::<_, AppError>(PositionResponse {
                    params: s.yield_source.params.into(),
                    balance: AssetsBalanceResponse {
                        balances: balance,
                        total_value,
                    },
                    liquidity,
                })
            })
            .collect::<Result<_, _>>()?,
    })
}

pub fn query_funds_value(
    deps: Deps,
    funds: Vec<Coin>,
    app: &App,
) -> AppResult<AssetsBalanceResponse> {
    let mut total_value = Uint128::zero();
    for fund in &funds {
        let exchange_rate = query_exchange_rate(deps, fund.denom.clone(), app)?;
        total_value += fund.amount * exchange_rate;
    }

    Ok(AssetsBalanceResponse {
        balances: funds,
        total_value,
    })
}

pub fn withdraw_share(
    deps: Deps,
    amount: Option<Uint128>,
    app: &App,
) -> AppResult<Option<Decimal>> {
    amount
        .map(|value| {
            let total_deposit = query_balance(deps, app)?;

            if total_deposit.total_value.is_zero() {
                return Err(AppError::NoDeposit {});
            }
            Ok(Decimal::from_ratio(value, total_deposit.total_value))
        })
        .transpose()
}
