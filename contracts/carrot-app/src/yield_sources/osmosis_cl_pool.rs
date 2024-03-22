use std::str::FromStr;

use crate::{
    contract::{App, AppResult},
    error::AppError,
    replies::{OSMOSIS_ADD_TO_POSITION_REPLY_ID, OSMOSIS_CREATE_POSITION_REPLY_ID},
    state::OSMOSIS_POSITION,
};
use abstract_app::traits::AccountIdentification;
use abstract_sdk::{AccountAction, Execution};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, CosmosMsg, Deps, Env, ReplyOn, SubMsg, Uint128};
use osmosis_std::{
    cosmwasm_to_proto_coins, try_proto_to_cosmwasm_coins,
    types::osmosis::concentratedliquidity::v1beta1::{
        ConcentratedliquidityQuerier, FullPositionBreakdown, MsgAddToPosition, MsgCreatePosition,
        MsgWithdrawPosition,
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
    let msg = app.executor(deps).execute_with_reply(
        vec![AccountAction::from_vec(vec![MsgCreatePosition {
            pool_id: params.pool_id,
            sender: proxy_addr.to_string(),
            lower_tick: params.lower_tick,
            upper_tick: params.upper_tick,
            tokens_provided: tokens,
            token_min_amount0: "0".to_string(),
            token_min_amount1: "0".to_string(),
        }])],
        ReplyOn::Success,
        OSMOSIS_CREATE_POSITION_REPLY_ID,
    )?;

    deps.api.debug("Created position messages");

    Ok(vec![msg])
}

fn raw_deposit(deps: Deps, funds: Vec<Coin>, app: &App) -> AppResult<Vec<SubMsg>> {
    let pool = get_osmosis_position(deps)?;
    let position = pool.position.unwrap();

    let proxy_addr = app.account_base(deps)?.proxy;
    let deposit_msg = app.executor(deps).execute_with_reply_and_data(
        MsgAddToPosition {
            position_id: position.position_id,
            sender: proxy_addr.to_string(),
            amount0: funds[0].amount.to_string(),
            amount1: funds[1].amount.to_string(),
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
    env: &Env,
    params: ConcentratedPoolParams,
    funds: Vec<Coin>,
    app: &App,
) -> AppResult<Vec<SubMsg>> {
    // We verify there is a position stored
    let osmosis_position = OSMOSIS_POSITION.may_load(deps.storage)?;
    if let Some(position) = osmosis_position {
        // We just deposit
        raw_deposit(deps, funds, app)
    } else {
        // We need to create a position
        create_position(deps, params, funds, app)
    }
}

pub fn withdraw(deps: Deps, amount: Option<Uint128>, app: &App) -> AppResult<Vec<CosmosMsg>> {
    let position = get_osmosis_position(deps)?;
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

pub fn user_deposit(deps: Deps, _app: &App) -> AppResult<Vec<Coin>> {
    let position = get_osmosis_position(deps)?;

    Ok([
        try_proto_to_cosmwasm_coins(position.asset0)?,
        try_proto_to_cosmwasm_coins(position.asset1)?,
    ]
    .into_iter()
    .flatten()
    .collect())
}

/// Returns an amount representing a user's liquidity
pub fn user_liquidity(deps: Deps, _app: &App) -> AppResult<Uint128> {
    let position = get_osmosis_position(deps)?;
    let total_liquidity = position.position.unwrap().liquidity.replace('.', "");

    Ok(Uint128::from_str(&total_liquidity)?)
}

pub fn user_rewards(deps: Deps, _app: &App) -> AppResult<Vec<Coin>> {
    let pool = get_osmosis_position(deps)?;

    let mut rewards = cosmwasm_std::Coins::default();
    for coin in try_proto_to_cosmwasm_coins(pool.claimable_incentives)? {
        rewards.add(coin)?;
    }

    for coin in try_proto_to_cosmwasm_coins(pool.claimable_spread_rewards)? {
        rewards.add(coin)?;
    }

    Ok(rewards.into())
}

// pub fn query_price(
//     deps: Deps,
//     funds: &[Coin],
//     app: &App,
//     max_spread: Option<Decimal>,
//     belief_price0: Option<Decimal>,
//     belief_price1: Option<Decimal>,
// ) -> AppResult<Decimal> {
//     let config = CONFIG.load(deps.storage)?;

//     let amount0 = funds
//         .iter()
//         .find(|c| c.denom == config.pool_config.token0)
//         .map(|c| c.amount)
//         .unwrap_or_default();
//     let amount1 = funds
//         .iter()
//         .find(|c| c.denom == config.pool_config.token1)
//         .map(|c| c.amount)
//         .unwrap_or_default();

//     // We take the biggest amount and simulate a swap for the corresponding asset
//     let price = if amount0 > amount1 {
//         let simulation_result = app.ans_dex(deps, OSMOSIS.to_string()).simulate_swap(
//             AnsAsset::new(config.pool_config.asset0, amount0),
//             config.pool_config.asset1,
//         )?;

//         let price = Decimal::from_ratio(amount0, simulation_result.return_amount);
//         if let Some(belief_price) = belief_price1 {
//             ensure!(
//                 belief_price.abs_diff(price) <= max_spread.unwrap_or(DEFAULT_SLIPPAGE),
//                 AppError::MaxSpreadAssertion { price }
//             );
//         }
//         price
//     } else {
//         let simulation_result = app.ans_dex(deps, OSMOSIS.to_string()).simulate_swap(
//             AnsAsset::new(config.pool_config.asset1, amount1),
//             config.pool_config.asset0,
//         )?;

//         let price = Decimal::from_ratio(simulation_result.return_amount, amount1);
//         if let Some(belief_price) = belief_price0 {
//             ensure!(
//                 belief_price.abs_diff(price) <= max_spread.unwrap_or(DEFAULT_SLIPPAGE),
//                 AppError::MaxSpreadAssertion { price }
//             );
//         }
//         price
//     };

//     Ok(price)
// }

#[cw_serde]
pub struct OsmosisPosition {
    pub position_id: u64,
}

pub fn get_position(deps: Deps) -> AppResult<OsmosisPosition> {
    OSMOSIS_POSITION
        .load(deps.storage)
        .map_err(|_| AppError::NoPosition {})
}

pub fn get_osmosis_position(deps: Deps) -> AppResult<FullPositionBreakdown> {
    let position = get_position(deps)?;

    ConcentratedliquidityQuerier::new(&deps.querier)
        .position_by_id(position.position_id)
        .map_err(|e| AppError::UnableToQueryPosition(position.position_id, e))?
        .position
        .ok_or(AppError::NoPosition {})
}
