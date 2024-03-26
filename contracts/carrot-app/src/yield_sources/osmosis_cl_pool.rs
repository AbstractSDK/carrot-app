use std::str::FromStr;

use crate::{
    contract::{App, AppResult},
    error::AppError,
    handlers::{query::query_exchange_rate, swap_helpers::DEFAULT_SLIPPAGE},
    replies::{OSMOSIS_ADD_TO_POSITION_REPLY_ID, OSMOSIS_CREATE_POSITION_REPLY_ID},
    state::CONFIG,
};
use abstract_app::{objects::AnsAsset, traits::AccountIdentification};
use abstract_dex_adapter::DexInterface;
use abstract_sdk::Execution;
use cosmwasm_std::{ensure, Coin, Coins, CosmosMsg, Decimal, Deps, Env, ReplyOn, SubMsg, Uint128};
use osmosis_std::{
    cosmwasm_to_proto_coins, try_proto_to_cosmwasm_coins,
    types::osmosis::concentratedliquidity::v1beta1::{
        ConcentratedliquidityQuerier, FullPositionBreakdown, MsgAddToPosition,
        MsgCollectIncentives, MsgCollectSpreadRewards, MsgCreatePosition, MsgWithdrawPosition,
    },
};

use super::yield_type::ConcentratedPoolParams;

/// This function creates a position for the user,
/// 1. Swap the indicated funds to match the asset0/asset1 ratio and deposit as much as possible in the pool for the given parameters
/// 2. Create a new position
/// 3. Store position id from create position response
///
/// * `lower_tick` - Concentrated liquidity pool parameter
/// * `upper_tick` - Concentrated liquidity pool parameter
/// * `funds` -  Funds that will be deposited from the user wallet directly into the pool. DO NOT SEND FUNDS TO THIS ENDPOINT
/// * `asset0` - The target amount of asset0.denom that the user will deposit inside the pool
/// * `asset1` - The target amount of asset1.denom that the user will deposit inside the pool
///
/// asset0 and asset1 are only used in a ratio to each other. They are there to make sure that the deposited funds will ALL land inside the pool.
/// We don't use an asset ratio because either one of the amounts can be zero
/// See https://docs.osmosis.zone/osmosis-core/modules/concentrated-liquidity for more details
///
fn create_position(
    deps: Deps,
    params: ConcentratedPoolParams,
    funds: Vec<Coin>,
    app: &App,
    // create_position_msg: CreatePositionMessage,
) -> AppResult<Vec<SubMsg>> {
    let proxy_addr = app.account_base(deps)?.proxy;

    // 2. Create a position
    let tokens = cosmwasm_to_proto_coins(funds);
    let msg = app.executor(deps).execute_with_reply_and_data(
        MsgCreatePosition {
            pool_id: params.pool_id,
            sender: proxy_addr.to_string(),
            lower_tick: params.lower_tick,
            upper_tick: params.upper_tick,
            tokens_provided: tokens,
            token_min_amount0: "0".to_string(),
            token_min_amount1: "0".to_string(),
        }
        .into(),
        ReplyOn::Success,
        OSMOSIS_CREATE_POSITION_REPLY_ID,
    )?;

    Ok(vec![msg])
}

fn raw_deposit(
    deps: Deps,
    funds: Vec<Coin>,
    app: &App,
    position_id: u64,
) -> AppResult<Vec<SubMsg>> {
    let pool = get_osmosis_position_by_id(deps, position_id)?;
    let position = pool.position.unwrap();

    let proxy_addr = app.account_base(deps)?.proxy;

    // We need to make sure the amounts are in the right order
    // We assume the funds vector has 2 coins associated
    let (amount0, amount1) = match pool
        .asset0
        .map(|c| c.denom == funds[0].denom)
        .or(pool.asset1.map(|c| c.denom == funds[1].denom))
    {
        Some(true) => (funds[0].amount, funds[1].amount), // we already had the right order
        Some(false) => (funds[1].amount, funds[0].amount), // we had the wrong order
        None => return Err(AppError::NoPosition {}), // A position has to exist in order to execute this function. This should be unreachable
    };

    let deposit_msg = app.executor(deps).execute_with_reply_and_data(
        MsgAddToPosition {
            position_id: position.position_id,
            sender: proxy_addr.to_string(),
            amount0: amount0.to_string(),
            amount1: amount1.to_string(),
            token_min_amount0: "0".to_string(),
            token_min_amount1: "0".to_string(),
        }
        .into(),
        cosmwasm_std::ReplyOn::Success,
        OSMOSIS_ADD_TO_POSITION_REPLY_ID,
    )?;

    Ok(vec![deposit_msg])
}

pub fn deposit(
    deps: Deps,
    _env: &Env,
    params: ConcentratedPoolParams,
    funds: Vec<Coin>,
    app: &App,
) -> AppResult<Vec<SubMsg>> {
    // We verify there is a position stored

    let osmosis_position = params
        .position_id
        .map(|position_id| get_osmosis_position_by_id(deps, position_id));

    if let Some(Ok(_)) = osmosis_position {
        // We just deposit
        raw_deposit(deps, funds, app, params.position_id.unwrap())
    } else {
        // We need to create a position
        create_position(deps, params, funds, app)
    }
}

pub fn withdraw(
    deps: Deps,
    amount: Option<Uint128>,
    app: &App,
    params: ConcentratedPoolParams,
) -> AppResult<Vec<CosmosMsg>> {
    let position =
        get_osmosis_position_by_id(deps, params.position_id.ok_or(AppError::NoPosition {})?)?;
    let position_details = position.position.unwrap();

    let total_liquidity = position_details.liquidity.replace('.', "");

    let liquidity_amount = if let Some(amount) = amount {
        amount.to_string()
    } else {
        // TODO: it's decimals inside contracts
        total_liquidity.clone()
    };
    let user = app.account_base(deps)?.proxy;

    // We need to execute withdraw on the user's behalf
    Ok(vec![MsgWithdrawPosition {
        position_id: position_details.position_id,
        sender: user.to_string(),
        liquidity_amount: liquidity_amount.clone(),
    }
    .into()])
}

pub fn withdraw_rewards(
    deps: Deps,
    params: ConcentratedPoolParams,
    app: &App,
) -> AppResult<(Vec<Coin>, Vec<CosmosMsg>)> {
    let position =
        get_osmosis_position_by_id(deps, params.position_id.ok_or(AppError::NoPosition {})?)?;
    let position_details = position.position.unwrap();

    let user = app.account_base(deps)?.proxy;
    let mut rewards = Coins::default();
    let mut msgs: Vec<CosmosMsg> = vec![];
    // If there are external incentives, claim them.
    if !position.claimable_incentives.is_empty() {
        for coin in try_proto_to_cosmwasm_coins(position.claimable_incentives)? {
            rewards.add(coin)?;
        }
        msgs.push(
            MsgCollectIncentives {
                position_ids: vec![position_details.position_id],
                sender: user.to_string(),
            }
            .into(),
        );
    }

    // If there is income from swap fees, claim them.
    if !position.claimable_spread_rewards.is_empty() {
        for coin in try_proto_to_cosmwasm_coins(position.claimable_spread_rewards)? {
            rewards.add(coin)?;
        }
        msgs.push(
            MsgCollectSpreadRewards {
                position_ids: vec![position_details.position_id],
                sender: position_details.address.clone(),
            }
            .into(),
        )
    }

    Ok((rewards.to_vec(), msgs))
}

/// This computes the current shares between assets in the position
/// For osmosis, it fetches the position and returns the current asset value ratio between assets
/// This will be called everytime when analyzing the current strategy, even if the position doesn't exist
/// This function should not error if the position doesn't exist
pub fn current_share(
    deps: Deps,
    shares: Vec<(String, Decimal)>,
    params: &ConcentratedPoolParams,
    app: &App,
) -> AppResult<Vec<(String, Decimal)>> {
    let position_id = if let Some(position_id) = params.position_id {
        position_id
    } else {
        // No position ? --> We return the target strategy
        return Ok(shares);
    };

    let position = if let Ok(position) = get_osmosis_position_by_id(deps, position_id) {
        position
    } else {
        // No position ? --> We return the target strategy
        return Ok(shares);
    };

    let (denom0, value0) = if let Some(asset) = position.asset0 {
        let exchange_rate = query_exchange_rate(deps, asset.denom.clone(), app)?;
        let value = Uint128::from_str(&asset.amount)? * exchange_rate;
        (Some(asset.denom), value)
    } else {
        (None, Uint128::zero())
    };

    let (denom1, value1) = if let Some(asset) = position.asset1 {
        let exchange_rate = query_exchange_rate(deps, asset.denom.clone(), app)?;
        let value = Uint128::from_str(&asset.amount)? * exchange_rate;
        (Some(asset.denom), value)
    } else {
        (None, Uint128::zero())
    };

    let total_value = value0 + value1;
    // No value ? --> We return the target strategy
    // This should be unreachable
    if total_value.is_zero() {
        return Ok(shares);
    }

    if denom0.is_none() {
        // If the first denom has no coins, all the value is in the second denom
        Ok(vec![(denom1.unwrap(), Decimal::one())])
    } else if denom1.is_none() {
        // If the second denom has no coins, all the value is in the first denom
        Ok(vec![(denom0.unwrap(), Decimal::one())])
    } else {
        Ok(vec![
            (denom0.unwrap(), Decimal::from_ratio(value0, total_value)),
            (denom1.unwrap(), Decimal::from_ratio(value1, total_value)),
        ])
    }
}

pub fn user_deposit(
    deps: Deps,
    _app: &App,
    params: ConcentratedPoolParams,
) -> AppResult<Vec<Coin>> {
    let position =
        get_osmosis_position_by_id(deps, params.position_id.ok_or(AppError::NoPosition {})?)?;

    Ok([
        try_proto_to_cosmwasm_coins(position.asset0)?,
        try_proto_to_cosmwasm_coins(position.asset1)?,
    ]
    .into_iter()
    .flatten()
    .collect())
}

/// Returns an amount representing a user's liquidity
pub fn user_liquidity(
    deps: Deps,
    _app: &App,
    params: ConcentratedPoolParams,
) -> AppResult<Uint128> {
    let position =
        get_osmosis_position_by_id(deps, params.position_id.ok_or(AppError::NoPosition {})?)?;
    let total_liquidity = position.position.unwrap().liquidity.replace('.', "");

    Ok(Uint128::from_str(&total_liquidity)?)
}

pub fn user_rewards(
    deps: Deps,
    _app: &App,
    params: ConcentratedPoolParams,
) -> AppResult<Vec<Coin>> {
    let position =
        get_osmosis_position_by_id(deps, params.position_id.ok_or(AppError::NoPosition {})?)?;

    let mut rewards = cosmwasm_std::Coins::default();
    for coin in try_proto_to_cosmwasm_coins(position.claimable_incentives)? {
        rewards.add(coin)?;
    }

    for coin in try_proto_to_cosmwasm_coins(position.claimable_spread_rewards)? {
        rewards.add(coin)?;
    }

    Ok(rewards.into())
}

pub fn query_swap_price(
    deps: Deps,
    app: &App,
    max_spread: Option<Decimal>,
    belief_price0: Option<Decimal>,
    belief_price1: Option<Decimal>,
    asset0: AnsAsset,
    asset1: AnsAsset,
) -> AppResult<Decimal> {
    let config = CONFIG.load(deps.storage)?;

    // We take the biggest amount and simulate a swap for the corresponding asset
    let price = if asset0.amount > asset1.amount {
        let simulation_result = app
            .ans_dex(deps, config.dex.clone())
            .simulate_swap(asset0.clone(), asset1.name)?;

        let price = Decimal::from_ratio(asset0.amount, simulation_result.return_amount);
        if let Some(belief_price) = belief_price1 {
            ensure!(
                belief_price.abs_diff(price) <= max_spread.unwrap_or(DEFAULT_SLIPPAGE),
                AppError::MaxSpreadAssertion { price }
            );
        }
        price
    } else {
        let simulation_result = app
            .ans_dex(deps, config.dex.clone())
            .simulate_swap(asset1.clone(), asset0.name)?;

        let price = Decimal::from_ratio(simulation_result.return_amount, asset1.amount);
        if let Some(belief_price) = belief_price0 {
            ensure!(
                belief_price.abs_diff(price) <= max_spread.unwrap_or(DEFAULT_SLIPPAGE),
                AppError::MaxSpreadAssertion { price }
            );
        }
        price
    };

    Ok(price)
}

pub fn get_osmosis_position_by_id(
    deps: Deps,
    position_id: u64,
) -> AppResult<FullPositionBreakdown> {
    ConcentratedliquidityQuerier::new(&deps.querier)
        .position_by_id(position_id)
        .map_err(|e| AppError::UnableToQueryPosition(position_id, e))?
        .position
        .ok_or(AppError::NoPosition {})
}
