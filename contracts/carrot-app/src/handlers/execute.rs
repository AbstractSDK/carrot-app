use super::swap_helpers::{swap_msg, swap_to_enter_position};
use crate::{
    contract::{App, AppResult, OSMOSIS},
    error::AppError,
    helpers::{get_balance, get_user},
    msg::{AppExecuteMsg, CreatePositionMessage, ExecuteMsg},
    replies::{ADD_TO_POSITION_ID, CREATE_POSITION_ID},
    state::{
        assert_contract, get_osmosis_position, get_position, get_position_status, Config, CONFIG,
    },
};
use abstract_app::abstract_sdk::AuthZInterface;
use abstract_app::{abstract_sdk::features::AbstractResponse, objects::AnsAsset};
use abstract_dex_adapter::DexInterface;
use abstract_sdk::{features::AbstractNameService, Resolve};
use cosmwasm_std::{
    to_json_binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, SubMsg, Uint128,
    WasmMsg,
};
use cw_asset::Asset;
use osmosis_std::{
    cosmwasm_to_proto_coins, try_proto_to_cosmwasm_coins,
    types::osmosis::concentratedliquidity::v1beta1::{
        MsgAddToPosition, MsgCollectIncentives, MsgCollectSpreadRewards, MsgCreatePosition,
        MsgWithdrawPosition,
    },
};
use std::str::FromStr;

pub fn execute_handler(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    app: App,
    msg: AppExecuteMsg,
) -> AppResult {
    match msg {
        AppExecuteMsg::CreatePosition(create_position_msg) => {
            create_position(deps, env, info, app, create_position_msg)
        }
        AppExecuteMsg::Deposit {
            funds,
            max_spread,
            belief_price0,
            belief_price1,
        } => deposit(
            deps,
            env,
            info,
            funds,
            max_spread,
            belief_price0,
            belief_price1,
            app,
        ),
        AppExecuteMsg::Withdraw { amount } => withdraw(deps, env, info, Some(amount), app),
        AppExecuteMsg::WithdrawAll {} => withdraw(deps, env, info, None, app),
        AppExecuteMsg::Autocompound {} => autocompound(deps, env, info, app),
    }
}

/// In this function, we want to create a new position for the user.
/// This operation happens in multiple steps:
/// 1. Withdraw a potential existing position and add the funds to the current position being created
/// 2. Create a new position using the existing funds (if any) + the funds that the user wishes to deposit additionally
fn create_position(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    app: App,
    create_position_msg: CreatePositionMessage,
) -> AppResult {
    // TODO verify authz permissions before creating the position
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;
    // We start by checking if there is already a position
    if get_osmosis_position(deps.as_ref()).is_ok() {
        return Err(AppError::PositionExists {});
        // If the position still has incentives to claim, the user is able to override it
    };

    let (swap_messages, create_position_msg) =
        _create_position(deps.as_ref(), &env, &app, create_position_msg)?;

    Ok(app
        .response("create_position")
        .add_messages(swap_messages)
        .add_submessage(create_position_msg))
}

#[allow(clippy::too_many_arguments)]
fn deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    funds: Vec<Coin>,
    max_spread: Option<Decimal>,
    belief_price0: Option<Decimal>,
    belief_price1: Option<Decimal>,
    app: App,
) -> AppResult {
    // Only the admin (manager contracts or account owner) + the smart contract can deposit
    app.admin
        .assert_admin(deps.as_ref(), &info.sender)
        .or(assert_contract(&info, &env))?;

    let pool = get_osmosis_position(deps.as_ref())?;
    let position = pool.position.unwrap();

    let asset0 = try_proto_to_cosmwasm_coins(pool.asset0.clone())?[0].clone();
    let asset1 = try_proto_to_cosmwasm_coins(pool.asset1.clone())?[0].clone();

    // When depositing, we start by adapting the available funds to the expected pool funds ratio
    // We do so by computing the swap information

    let (swap_msgs, resulting_assets) = swap_to_enter_position(
        deps.as_ref(),
        &env,
        funds,
        &app,
        asset0,
        asset1,
        max_spread,
        belief_price0,
        belief_price1,
    )?;

    let user = get_user(deps.as_ref(), &app)?;

    let deposit_msg = app.auth_z(deps.as_ref(), Some(user.clone()))?.execute(
        &env.contract.address,
        MsgAddToPosition {
            position_id: position.position_id,
            sender: user.to_string(),
            amount0: resulting_assets[0].amount.to_string(),
            amount1: resulting_assets[1].amount.to_string(),
            token_min_amount0: "0".to_string(),
            token_min_amount1: "0".to_string(),
        },
    );

    Ok(app
        .response("deposit")
        .add_messages(swap_msgs)
        .add_submessage(SubMsg::reply_on_success(deposit_msg, ADD_TO_POSITION_ID)))
}

fn withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Option<Uint128>,
    app: App,
) -> AppResult {
    // Only the authorized addresses (admin ?) can withdraw
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;

    let (withdraw_msg, withdraw_amount, total_amount, _withdrawn_funds) =
        _inner_withdraw(deps, &env, amount, &app)?;

    Ok(app
        .response("withdraw")
        .add_attribute("withdraw_amount", withdraw_amount)
        .add_attribute("total_amount", total_amount)
        .add_message(withdraw_msg))
}

/// Auto-compound the position with earned fees and incentives.

fn autocompound(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Everyone can autocompound

    let position = get_osmosis_position(deps.as_ref())?;
    let position_details = position.position.unwrap();

    let mut rewards = cosmwasm_std::Coins::default();
    let mut collect_rewards_msgs = vec![];

    // Get app's user and set up authz.
    let user = get_user(deps.as_ref(), &app)?;
    let authz = app.auth_z(deps.as_ref(), Some(user.clone()))?;

    // If there are external incentives, claim them.
    if !position.claimable_incentives.is_empty() {
        let asset0_denom = position.asset0.unwrap().denom;
        let asset1_denom = position.asset1.unwrap().denom;

        for coin in try_proto_to_cosmwasm_coins(position.claimable_incentives)? {
            if coin.denom == asset0_denom || coin.denom == asset1_denom {
                rewards.add(coin)?;
            }
        }
        collect_rewards_msgs.push(authz.execute(
            &env.contract.address,
            MsgCollectIncentives {
                position_ids: vec![position_details.position_id],
                sender: user.to_string(),
            },
        ));
    }

    // If there is income from swap fees, claim them.
    if !position.claimable_spread_rewards.is_empty() {
        for coin in try_proto_to_cosmwasm_coins(position.claimable_spread_rewards)? {
            rewards.add(coin)?;
        }
        collect_rewards_msgs.push(authz.execute(
            &env.contract.address,
            MsgCollectSpreadRewards {
                position_ids: vec![position_details.position_id],
                sender: position_details.address.clone(),
            },
        ))
    }

    // If there are no rewards, we can't do anything
    if rewards.is_empty() {
        return Err(crate::error::AppError::NoRewards {});
    }

    // Finally we deposit of all rewarded tokens into the position
    let msg_deposit = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::Deposit {
            funds: rewards.into(),
            max_spread: None,
            belief_price0: None,
            belief_price1: None,
        }))?,
        funds: vec![],
    });

    let mut response = app
        .response("auto-compound")
        .add_messages(collect_rewards_msgs)
        .add_message(msg_deposit);

    // If called by non-admin and reward cooldown has ended, send rewards to the contract caller.
    let config = CONFIG.load(deps.storage)?;
    if !app.admin.is_admin(deps.as_ref(), &info.sender)?
        && get_position_status(
            deps.storage,
            &env,
            config.autocompound_cooldown_seconds.u64(),
        )?
        .is_ready()
    {
        let executor_reward_messages = autocompound_executor_rewards(
            deps.as_ref(),
            &env,
            info.sender.into_string(),
            &app,
            config,
        )?;

        response = response.add_messages(executor_reward_messages);
    }

    Ok(response)
}

fn _inner_withdraw(
    deps: DepsMut,
    env: &Env,
    amount: Option<Uint128>,
    app: &App,
) -> AppResult<(CosmosMsg, String, String, Vec<Coin>)> {
    let position = get_osmosis_position(deps.as_ref())?;
    let position_details = position.position.unwrap();

    let total_liquidity = position_details.liquidity.replace('.', "");

    let liquidity_amount = if let Some(amount) = amount {
        amount.to_string()
    } else {
        // TODO: it's decimals inside contracts
        total_liquidity.clone()
    };
    let user = get_user(deps.as_ref(), app)?;

    // We need to execute withdraw on the user's behalf
    let msg = app.auth_z(deps.as_ref(), Some(user.clone()))?.execute(
        &env.contract.address,
        MsgWithdrawPosition {
            position_id: position_details.position_id,
            sender: user.to_string(),
            liquidity_amount: liquidity_amount.clone(),
        },
    );

    let withdrawn_funds = vec![
        try_proto_to_cosmwasm_coins(position.asset0)?
            .first()
            .map(|c| {
                Ok::<_, AppError>(Coin {
                    denom: c.denom.clone(),
                    amount: c.amount * Uint128::from_str(&liquidity_amount)?
                        / Uint128::from_str(&total_liquidity)?,
                })
            })
            .transpose()?,
        try_proto_to_cosmwasm_coins(position.asset1)?
            .first()
            .map(|c| {
                Ok::<_, AppError>(Coin {
                    denom: c.denom.clone(),
                    amount: c.amount * Uint128::from_str(&liquidity_amount)?
                        / Uint128::from_str(&total_liquidity)?,
                })
            })
            .transpose()?,
    ]
    .into_iter()
    .flatten()
    .collect();

    Ok((msg, liquidity_amount, total_liquidity, withdrawn_funds))
}

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
pub(crate) fn _create_position(
    deps: Deps,
    env: &Env,
    app: &App,
    create_position_msg: CreatePositionMessage,
) -> AppResult<(Vec<CosmosMsg>, SubMsg)> {
    let config = CONFIG.load(deps.storage)?;

    let CreatePositionMessage {
        lower_tick,
        upper_tick,
        funds,
        asset0,
        asset1,
        max_spread,
        belief_price0,
        belief_price1,
    } = create_position_msg;

    // 1. Swap the assets
    let (swap_msgs, resulting_assets) = swap_to_enter_position(
        deps,
        env,
        funds,
        app,
        asset0,
        asset1,
        max_spread,
        belief_price0,
        belief_price1,
    )?;
    let sender = get_user(deps, app)?;

    // 2. Create a position
    let tokens = cosmwasm_to_proto_coins(resulting_assets);
    let create_msg = app.auth_z(deps, Some(sender.clone()))?.execute(
        &env.contract.address,
        MsgCreatePosition {
            pool_id: config.pool_config.pool_id,
            sender: sender.to_string(),
            lower_tick,
            upper_tick,
            tokens_provided: tokens,
            token_min_amount0: "0".to_string(),
            token_min_amount1: "0".to_string(),
        },
    );

    Ok((
        swap_msgs,
        // 3. Use a reply to get the stored position id
        SubMsg::reply_on_success(create_msg, CREATE_POSITION_ID),
    ))
}

/// Sends autocompound rewards to the executor.
/// In case user does not have not enough gas token the contract will swap some
/// tokens for gas tokens.
pub fn autocompound_executor_rewards(
    deps: Deps,
    env: &Env,
    executor: String,
    app: &App,
    config: Config,
) -> AppResult<Vec<CosmosMsg>> {
    let rewards_config = config.autocompound_rewards_config;
    let position = get_position(deps)?;
    let user = position.owner;

    // Get user balance of gas denom
    let gas_denom = rewards_config
        .gas_asset
        .resolve(&deps.querier, &app.ans_host(deps)?)?;
    let user_gas_balance = gas_denom.query_balance(&deps.querier, user.clone())?;

    let mut rewards_messages = vec![];

    // If not enough gas coins - swap for some amount
    if user_gas_balance < rewards_config.min_gas_balance {
        // Get asset entries
        let dex = app.ans_dex(deps, OSMOSIS.to_string());

        // Do reverse swap to find approximate amount we need to swap
        let need_gas_coins = rewards_config.max_gas_balance - user_gas_balance;
        let simulate_swap_response = dex.simulate_swap(
            AnsAsset::new(rewards_config.gas_asset.clone(), need_gas_coins),
            rewards_config.swap_asset.clone(),
        )?;

        // Get user balance of swap denom
        let user_swap_balance =
            get_balance(rewards_config.swap_asset.clone(), deps, user.clone(), app)?;

        // Swap as much as available if not enough for max_gas_balance
        let swap_amount = simulate_swap_response.return_amount.min(user_swap_balance);

        let msgs = swap_msg(
            deps,
            env,
            AnsAsset::new(rewards_config.swap_asset, swap_amount),
            rewards_config.gas_asset,
            app,
        )?;
        rewards_messages.extend(msgs);
    }

    let reward_asset = Asset::new(gas_denom, rewards_config.reward);
    let msg_send = reward_asset.transfer_msg(env.contract.address.to_string())?;

    // To avoid giving general `MsgSend` authorization to any address we do 2 sends here
    // 1) From user to the contract
    // 2) From contract to the executor
    // That way we can limit the `MsgSend` authorization to the contract address only.
    let send_reward_to_contract_msg = app
        .auth_z(deps, Some(cosmwasm_std::Addr::unchecked(user)))?
        .execute(&env.contract.address, msg_send);
    rewards_messages.push(send_reward_to_contract_msg);

    let send_reward_to_executor_msg = reward_asset.transfer_msg(executor)?;

    rewards_messages.push(send_reward_to_executor_msg);

    Ok(rewards_messages)
}
