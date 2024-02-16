use super::swap_helpers::{swap_msg, swap_to_enter_position};
use crate::{
    contract::{App, AppResult},
    helpers::get_user,
    msg::{AppExecuteMsg, CreatePositionMessage, ExecuteMsg},
    replies::{ADD_TO_POSITION_ID, CREATE_POSITION_ID},
    state::{
        assert_contract, get_osmosis_position, get_position, get_position_status, CONFIG, POSITION,
    },
};
use abstract_app::abstract_sdk::AuthZInterface;
use abstract_app::AppError;
use abstract_app::{abstract_sdk::features::AbstractResponse, objects::AnsAsset};
use abstract_dex_adapter::DexInterface;
use abstract_sdk::{features::AbstractNameService, Resolve};
use cosmwasm_std::{
    to_json_binary, BankMsg, Coin, CosmosMsg, DepsMut, Env, MessageInfo, SubMsg, Uint128, WasmMsg,
};
use cosmwasm_std::{Coins, Deps};
use cw_asset::AssetInfo;
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
        AppExecuteMsg::Deposit { funds } => deposit(deps, env, info, funds, app),
        AppExecuteMsg::Withdraw { amount } => withdraw(deps, env, info, Some(amount), app),
        AppExecuteMsg::WithdrawAll {} => withdraw(deps, env, info, None, app),
        AppExecuteMsg::Autocompound {} => autocompound(deps, env, info, app),
    }
}

fn create_position(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    app: App,
    mut create_position_msg: CreatePositionMessage,
) -> AppResult {
    // TODO verify authz permissions before creating the position
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;
    let mut response = app.response("create_position");
    // We start by checking if there is already a position
    let funds = create_position_msg.funds;
    let funds_to_deposit = if POSITION.exists(deps.storage) {
        let (withdraw_msg, withdraw_amount, total_amount, withdrawn_funds) =
            _inner_withdraw(deps.as_ref(), &env, None, &app)?;

        response = response
            .add_message(withdraw_msg)
            .add_attribute("withdraw_amount", withdraw_amount)
            .add_attribute("total_amount", total_amount);

        // We add the withdrawn funds to the input funds
        let mut coins: Coins = funds.try_into()?;
        for fund in withdrawn_funds {
            coins.add(fund)?;
        }
        POSITION.remove(deps.storage);
        coins.to_vec()
    } else {
        funds
    };

    create_position_msg.funds = funds_to_deposit;
    let (swap_messages, create_position_msg) =
        _create_position(deps.as_ref(), &env, &app, create_position_msg)?;

    Ok(response
        .add_messages(swap_messages)
        .add_submessage(create_position_msg))
}

fn deposit(deps: DepsMut, env: Env, info: MessageInfo, funds: Vec<Coin>, app: App) -> AppResult {
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

    let (swap_msgs, resulting_assets) =
        swap_to_enter_position(deps.as_ref(), &env, funds, &app, asset0, asset1)?;

    let user = get_user(deps.as_ref(), &app)?;

    let deposit_msg = app.auth_z(deps.as_ref(), Some(user.clone()))?.execute(
        &env.contract.address,
        MsgAddToPosition {
            position_id: position.position_id,
            sender: user.to_string(),
            amount0: resulting_assets[0].amount.to_string(),
            amount1: resulting_assets[1].amount.to_string(),
            token_min_amount0: "0".to_string(), // No min, this always works
            token_min_amount1: "0".to_string(), // No min, this always works
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
        _inner_withdraw(deps.as_ref(), &env, amount, &app)?;

    Ok(app
        .response("withdraw")
        .add_attribute("withdraw_amount", withdraw_amount)
        .add_attribute("total_amount", total_amount)
        .add_message(withdraw_msg))
}

fn autocompound(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Everyone can autocompound
    let config = CONFIG.load(deps.storage)?;

    let position = get_osmosis_position(deps.as_ref())?;
    let position_details = position.position.unwrap();

    let mut rewards = cosmwasm_std::Coins::default();
    let mut collect_rewards_msgs = vec![];

    let user = get_user(deps.as_ref(), &app)?;
    let authz = app.auth_z(deps.as_ref(), Some(user.clone()))?;
    if !position.claimable_incentives.is_empty() {
        for coin in try_proto_to_cosmwasm_coins(position.claimable_incentives)? {
            rewards.add(coin)?;
        }
        collect_rewards_msgs.push(authz.execute(
            &env.contract.address,
            MsgCollectIncentives {
                position_ids: vec![position_details.position_id],
                sender: user.to_string(),
            },
        ));
    }

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

    if rewards.is_empty() {
        return Err(crate::error::AppError::NoRewards {});
    }

    // Finally we ask for a deposit of all rewards from the wallet to the position
    let msg_deposit = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::Deposit {
            funds: rewards.into(),
        }))?,
        funds: vec![],
    });

    let mut response = app
        .response("auto-compound")
        .add_messages(collect_rewards_msgs)
        .add_message(msg_deposit);

    // If called by non-admin and savings app ready to give rewards to the sender - send rewards to the sender
    if !app.admin.is_admin(deps.as_ref(), &info.sender)?
        && get_position_status(
            deps.storage,
            &env,
            config.autocompound_cooldown_seconds.u64(),
        )?
        .is_ready()
    {
        let executor_reward_messages =
            autocompound_executor_rewards(deps.as_ref(), &env, info.sender.into_string(), &app)?;

        response = response.add_messages(executor_reward_messages);
    }

    Ok(response)
}

fn _inner_withdraw(
    deps: Deps,
    env: &Env,
    amount: Option<Uint128>,
    app: &App,
) -> AppResult<(CosmosMsg, String, String, Vec<Coin>)> {
    let position = get_osmosis_position(deps)?;
    let position_details = position.position.unwrap();

    let total_liquidity = position_details.liquidity.replace('.', "");

    let liquidity_amount = if let Some(amount) = amount {
        amount.to_string()
    } else {
        // TODO: it's decimals inside contracts
        total_liquidity.clone()
    };
    let user = get_user(deps, app)?;

    // We need to execute withdraw on the user's behalf
    let msg = app.auth_z(deps, Some(user.clone()))?.execute(
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
    } = create_position_msg;

    // With the current funds, we need to be able to create a position that makes sense
    // Therefore we swap the incoming funds to fit inside the future position
    let (swap_msgs, resulting_assets) =
        swap_to_enter_position(deps, env, funds, app, asset0, asset1)?;

    let sender = get_user(deps, app)?;

    let create_msg = app.auth_z(deps, Some(sender.clone()))?.execute(
        &env.contract.address,
        MsgCreatePosition {
            pool_id: config.pool_config.pool_id,
            sender: sender.to_string(),
            lower_tick,
            upper_tick,
            tokens_provided: cosmwasm_to_proto_coins(resulting_assets),
            token_min_amount0: "0".to_string(), // No min amount here
            token_min_amount1: "0".to_string(), // No min amount, we want to deposit whatever we can
        },
    );

    Ok((
        swap_msgs,
        SubMsg::reply_always(create_msg, CREATE_POSITION_ID),
    ))
}

/// Sends autocompound rewards to the executor.
/// In case user have not enough rewards it will also do a swap
pub fn autocompound_executor_rewards(
    deps: Deps,
    env: &Env,
    executor: String,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
    let config = CONFIG.load(deps.storage)?;
    let rewards_config = config.autocompound_rewards_config;
    let position = get_position(deps)?;
    let user = position.owner;

    // Get user balance of gas denom
    let user_gas_balance = deps
        .querier
        .query_balance(user.clone(), rewards_config.gas_denom.clone())?;

    let mut rewards_messages = vec![];

    // If not enough gas coins - swap for some amount
    if user_gas_balance.amount < rewards_config.min_gas_balance {
        // Get asset entries
        let dex = app.dex(deps, config.exchange);
        let ans_host = app.ans_host(deps)?;
        let gas_asset = AssetInfo::Native(rewards_config.gas_denom.clone())
            .resolve(&deps.querier, &ans_host)?;
        let swap_asset = AssetInfo::Native(rewards_config.swap_denom.clone())
            .resolve(&deps.querier, &ans_host)?;

        // Do reverse swap to find approximate amount we need to swap
        let need_gas_coins = rewards_config.max_gas_balance - user_gas_balance.amount;
        let simulate_swap_response = dex.simulate_swap(
            AnsAsset::new(gas_asset.clone(), need_gas_coins),
            swap_asset.clone(),
        )?;

        // Get user balance of swap denom
        let user_swap_balance = deps
            .querier
            .query_balance(user.clone(), rewards_config.swap_denom)?;

        // Swap as much as available if not enough for max_gas_balance
        let swap_amount = simulate_swap_response
            .return_amount
            .min(user_swap_balance.amount);

        let msgs = swap_msg(
            deps,
            env,
            AnsAsset::new(swap_asset, swap_amount),
            gas_asset,
            app,
        )?;
        rewards_messages.extend(msgs);
    }

    let reward = Coin {
        denom: rewards_config.gas_denom,
        amount: rewards_config.reward,
    };
    // To avoid giving general `MsgSend` authorization we do 2 sends here
    // 1) From user to the contract
    // 2) From contract to the executor
    let msg_send = BankMsg::Send {
        to_address: env.contract.address.to_string(),
        amount: vec![reward.clone()],
    };
    let reward_into_contract = app
        .auth_z(deps, Some(cosmwasm_std::Addr::unchecked(user)))?
        .execute(&env.contract.address, msg_send);
    rewards_messages.push(reward_into_contract);

    let reward_into_executor = BankMsg::Send {
        to_address: executor,
        amount: vec![reward],
    };
    rewards_messages.push(reward_into_executor.into());

    Ok(rewards_messages)
}
