use abstract_app::traits::AccountIdentification;
use abstract_app::{
    abstract_core::objects::AnsAsset,
    traits::{AbstractNameService, Resolve},
};
use abstract_dex_adapter::DexInterface;
use cosmwasm_std::{to_json_binary, Binary, Coins, Deps, Env, Uint128};
use cw_asset::Asset;

use crate::autocompound::get_autocompound_status;
use crate::exchange_rate::query_exchange_rate;
use crate::msg::{PositionResponse, PositionsResponse};
use crate::{
    contract::{App, AppResult},
    error::AppError,
    helpers::get_balance,
    msg::{
        AppQueryMsg, AssetsBalanceResponse, AvailableRewardsResponse, CompoundStatusResponse,
        StrategyResponse,
    },
    state::{Config, CONFIG},
};

pub fn query_handler(deps: Deps, env: Env, app: &App, msg: AppQueryMsg) -> AppResult<Binary> {
    match msg {
        AppQueryMsg::Balance {} => to_json_binary(&query_balance(deps, app)?),
        AppQueryMsg::AvailableRewards {} => to_json_binary(&query_rewards(deps, app)?),
        AppQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        AppQueryMsg::Strategy {} => to_json_binary(&query_strategy(deps)?),
        AppQueryMsg::CompoundStatus {} => to_json_binary(&query_compound_status(deps, env, app)?),
        AppQueryMsg::RebalancePreview {} => todo!(),
        AppQueryMsg::StrategyStatus {} => to_json_binary(&query_strategy_status(deps, app)?),
        AppQueryMsg::Positions {} => to_json_binary(&query_positions(deps, app)?),
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

    let gas_denom = config
        .autocompound_config
        .rewards
        .gas_asset
        .resolve(&deps.querier, &app.ans_host(deps)?)?;

    let reward = Asset::new(gas_denom.clone(), config.autocompound_config.rewards.reward);

    let user = app.account_base(deps)?.proxy;

    let user_gas_balance = gas_denom.query_balance(&deps.querier, user.clone())?;

    let rewards_available = if user_gas_balance >= reward.amount {
        true
    } else {
        // check if can swap
        let rewards_config = config.autocompound_config.rewards;
        let dex = app.ans_dex(deps, config.dex);

        // Reverse swap to see how many swap coins needed
        let required_gas_coins = reward.amount - user_gas_balance;
        let response = dex.simulate_swap(
            AnsAsset::new(rewards_config.gas_asset, required_gas_coins),
            rewards_config.swap_asset.clone(),
        )?;

        // Check if user has enough of swap coins
        let user_swap_balance = get_balance(rewards_config.swap_asset, deps, user, app)?;
        let required_swap_amount = response.return_amount;

        user_swap_balance > required_swap_amount
    };

    Ok(CompoundStatusResponse {
        status,
        reward: reward.into(),
        rewards_available,
    })
}

pub fn query_strategy(deps: Deps) -> AppResult<StrategyResponse> {
    let config = CONFIG.load(deps.storage)?;

    Ok(StrategyResponse {
        strategy: config.balance_strategy,
    })
}

pub fn query_strategy_status(deps: Deps, app: &App) -> AppResult<StrategyResponse> {
    let strategy = query_strategy(deps)?.strategy;

    Ok(StrategyResponse {
        strategy: strategy.query_current_status(deps, app)?,
    })
}

fn query_config(deps: Deps) -> AppResult<Config> {
    Ok(CONFIG.load(deps.storage)?)
}

pub fn query_balance(deps: Deps, app: &App) -> AppResult<AssetsBalanceResponse> {
    let mut funds = Coins::default();
    let mut total_value = Uint128::zero();
    query_strategy(deps)?.strategy.0.iter().try_for_each(|s| {
        let deposit_value = s
            .yield_source
            .ty
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
    let strategy = query_strategy(deps)?.strategy;

    let mut rewards = Coins::default();
    strategy.0.into_iter().try_for_each(|s| {
        let this_rewards = s.yield_source.ty.user_rewards(deps, app)?;
        for fund in this_rewards {
            rewards.add(fund)?;
        }
        Ok::<_, AppError>(())
    })?;

    Ok(AvailableRewardsResponse {
        available_rewards: rewards.into(),
    })
}

pub fn query_positions(deps: Deps, app: &App) -> AppResult<PositionsResponse> {
    Ok(PositionsResponse {
        positions: query_strategy(deps)?
            .strategy
            .0
            .into_iter()
            .map(|s| {
                let balance = s.yield_source.ty.user_deposit(deps, app)?;
                let liquidity = s.yield_source.ty.user_liquidity(deps, app)?;

                let total_value = balance
                    .iter()
                    .map(|fund| {
                        let exchange_rate = query_exchange_rate(deps, fund.denom.clone(), app)?;
                        Ok(fund.amount * exchange_rate)
                    })
                    .sum::<AppResult<Uint128>>()?;

                Ok::<_, AppError>(PositionResponse {
                    ty: s.yield_source.ty,
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
