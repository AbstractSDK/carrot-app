use super::swap_helpers::{swap_msg, swap_to_enter_position};
use crate::{
    contract::{App, AppResult, OSMOSIS},
    error::AppError,
    helpers::{get_balance, get_user, nonpayable},
    msg::{AppExecuteMsg, CreatePositionMessage, ExecuteMsg, SwapToAsset},
    replies::{ADD_TO_POSITION_ID, CREATE_POSITION_ID, WITHDRAW_TO_ASSET_ID},
    state::{
        assert_contract, AutocompoundRewardsConfig, CarrotPosition, Config, CONFIG,
        TEMP_WITHDRAW_TO_ASSET,
    },
};
use abstract_app::{
    objects::AnsAsset,
    sdk::{
        features::{AbstractNameService, AbstractResponse},
        AuthZ, AuthZInterface, Resolve,
    },
};
use abstract_dex_adapter::DexInterface;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, CosmosMsg, Decimal, Decimal256, Deps, DepsMut, Env, MessageInfo,
    SubMsg, Uint128, Uint256, Uint64, WasmMsg,
};
use cw_asset::Asset;
use osmosis_std::{
    try_proto_to_cosmwasm_coins,
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
    nonpayable(&info)?;

    match msg {
        AppExecuteMsg::UpdateConfig {
            autocompound_cooldown_seconds,
            autocompound_rewards_config,
        } => update_config(
            deps,
            app,
            autocompound_cooldown_seconds,
            autocompound_rewards_config,
        ),
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
        AppExecuteMsg::Withdraw { amount, swap_to } => {
            withdraw(deps, env, info, amount, swap_to, app)
        }
        AppExecuteMsg::Autocompound {} => autocompound(deps, env, info, app),
    }
}

fn update_config(
    deps: DepsMut,
    app: App,
    autocompound_cooldown_seconds: Option<Uint64>,
    autocompound_rewards_config: Option<AutocompoundRewardsConfig>,
) -> AppResult {
    let mut config = CONFIG.load(deps.storage)?;

    if let Some(new_rewards_config) = autocompound_rewards_config {
        // Validate rewards config first
        let ans = app.name_service(deps.as_ref());
        let asset_pairing_resp: Vec<abstract_app::std::ans_host::AssetPairingMapEntry> = ans
            .pool_list(
                Some(abstract_app::std::ans_host::AssetPairingFilter {
                    asset_pair: Some((
                        new_rewards_config.gas_asset.clone(),
                        new_rewards_config.swap_asset.clone(),
                    )),
                    dex: None,
                }),
                None,
                None,
            )?;

        let pair = asset_pairing_resp
            .into_iter()
            .find(|(_, refs)| !refs.is_empty())
            .ok_or(AppError::NoSwapPossibility {})?
            .0;
        let dex_name = pair.dex();
        new_rewards_config.check(deps.as_ref(), dex_name, ans.host())?;
        config.autocompound_rewards_config = new_rewards_config;
    }
    if let Some(new_autocompound_cooldown) = autocompound_cooldown_seconds {
        config.autocompound_cooldown_seconds = new_autocompound_cooldown;
    }

    CONFIG.save(deps.storage, &config)?;
    Ok(app.response("update_config"))
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
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;
    // Check if there is already saved position
    if CarrotPosition::may_load(deps.as_ref())?.is_some() {
        return Err(AppError::PositionExists {});
    }

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

    let carrot_position = CarrotPosition::load(deps.as_ref())?;

    let asset0: Coin = carrot_position.position.asset0.unwrap().try_into()?;
    let asset1: Coin = carrot_position.position.asset1.unwrap().try_into()?;

    // When depositing, we start by adapting the available funds to the expected pool funds ratio
    // We do so by computing the swap information

    let (swap_msgs, assets_for_position) = swap_to_enter_position(
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
            position_id: carrot_position.id,
            sender: user.to_string(),
            amount0: assets_for_position.asset0.amount.to_string(),
            amount1: assets_for_position.asset1.amount.to_string(),
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
    amount: Option<Uint256>,
    swap_to: Option<SwapToAsset>,
    app: App,
) -> AppResult {
    // Only the authorized addresses (admin ?) can withdraw
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;

    let carrot_position = CarrotPosition::load(deps.as_ref())?;
    // Get app's user and set up authz.
    let user = get_user(deps.as_ref(), &app)?;
    let authz = app.auth_z(deps.as_ref(), Some(user.clone()))?;

    // Collect all rewards/incentives if they exist
    let (collect_rewards_msgs, rewards) =
        _inner_claim_rewards(&env, carrot_position.clone(), user.clone(), authz.clone())?;

    // Withdraw funds
    let (withdraw_msg, withdraw_amount, total_amount, _withdrawn_funds) =
        _inner_withdraw(&env, amount, carrot_position, user, authz)?;

    let partial_withdraw = withdraw_amount != total_amount;

    let mut app_response = app
        .response("withdraw")
        .add_attribute("withdraw_amount", withdraw_amount)
        .add_attribute("total_amount", total_amount);

    // Resolve to_asset if provided
    app_response = if let Some(swap_to) = swap_to {
        TEMP_WITHDRAW_TO_ASSET.save(deps.storage, &swap_to)?;
        app_response.add_submessage(SubMsg::reply_on_success(withdraw_msg, WITHDRAW_TO_ASSET_ID))
    } else {
        app_response.add_message(withdraw_msg)
    };
    // Add the collect_rewards_msgs only if there are rewards AND if we are doing a partial withdraw
    // Context: While partial position withdraws on osmosis keep the rewards unclaimed, full withdraws automatically withdraw rewards
    if !rewards.is_empty() && partial_withdraw {
        app_response = app_response.add_messages(collect_rewards_msgs);
    }

    Ok(app_response)
}

/// Auto-compound the position with earned fees and incentives.
fn autocompound(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Everyone can autocompound
    let config = CONFIG.load(deps.storage)?;

    let (compound_status, maybe_carrot_position) = CarrotPosition::compound_status(
        deps.as_ref(),
        &env,
        config.autocompound_cooldown_seconds.u64(),
    )?;

    // Check if osmosis returned position
    let carrot_position = maybe_carrot_position.ok_or(AppError::NoPosition {})?;

    let mut rewards = cosmwasm_std::Coins::default();
    let mut collect_rewards_msgs = vec![];

    // Get app's user and set up authz.
    let user = get_user(deps.as_ref(), &app)?;
    let authz = app.auth_z(deps.as_ref(), Some(user.clone()))?;

    // If there are external incentives, claim them.
    if !carrot_position.position.claimable_incentives.is_empty() {
        let asset0_denom = carrot_position.position.asset0.unwrap().denom;
        let asset1_denom = carrot_position.position.asset1.unwrap().denom;

        for coin in try_proto_to_cosmwasm_coins(carrot_position.position.claimable_incentives)? {
            if coin.denom == asset0_denom || coin.denom == asset1_denom {
                rewards.add(coin)?;
            }
        }
        collect_rewards_msgs.push(authz.execute(
            &env.contract.address,
            MsgCollectIncentives {
                position_ids: vec![carrot_position.id],
                sender: user.to_string(),
            },
        ));
    }

    // If there is income from swap fees, claim them.
    if !carrot_position.position.claimable_spread_rewards.is_empty() {
        for coin in try_proto_to_cosmwasm_coins(carrot_position.position.claimable_spread_rewards)?
        {
            rewards.add(coin)?;
        }
        collect_rewards_msgs.push(authz.execute(
            &env.contract.address,
            MsgCollectSpreadRewards {
                position_ids: vec![carrot_position.id],
                sender: user.to_string(),
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
    if !app.admin.is_admin(deps.as_ref(), &info.sender)? && compound_status.is_ready() {
        let executor_reward_messages = autocompound_executor_rewards(
            deps.as_ref(),
            &env,
            info.sender.into_string(),
            &app,
            user,
            config,
        )?;

        response = response.add_messages(executor_reward_messages);
    }

    Ok(response)
}

fn _inner_claim_rewards(
    env: &Env,
    carrot_position: CarrotPosition,
    user: Addr,
    authz: AuthZ,
) -> AppResult<(Vec<CosmosMsg>, cosmwasm_std::Coins)> {
    let mut rewards = cosmwasm_std::Coins::default();
    let mut collect_rewards_msgs = vec![];

    // If there are external incentives, claim them.
    if !carrot_position.position.claimable_incentives.is_empty() {
        let asset0_denom = carrot_position.position.asset0.unwrap().denom;
        let asset1_denom = carrot_position.position.asset1.unwrap().denom;

        for coin in try_proto_to_cosmwasm_coins(carrot_position.position.claimable_incentives)? {
            if coin.denom == asset0_denom || coin.denom == asset1_denom {
                rewards.add(coin)?;
            }
        }
        collect_rewards_msgs.push(authz.execute(
            &env.contract.address,
            MsgCollectIncentives {
                position_ids: vec![carrot_position.id],
                sender: user.to_string(),
            },
        ));
    }

    // If there is income from swap fees, claim them.
    if !carrot_position.position.claimable_spread_rewards.is_empty() {
        for coin in try_proto_to_cosmwasm_coins(carrot_position.position.claimable_spread_rewards)?
        {
            rewards.add(coin)?;
        }
        collect_rewards_msgs.push(authz.execute(
            &env.contract.address,
            MsgCollectSpreadRewards {
                position_ids: vec![carrot_position.id],
                sender: user.to_string(),
            },
        ))
    }
    Ok((collect_rewards_msgs, rewards))
}

fn _inner_withdraw(
    env: &Env,
    amount: Option<Uint256>,
    carrot_position: CarrotPosition,
    user: Addr,
    authz: AuthZ,
) -> AppResult<(CosmosMsg, Uint256, Uint256, [Coin; 2])> {
    let position_details = carrot_position.position.position.unwrap();
    let total_liquidity: Decimal256 = position_details.liquidity.parse()?;
    let total_liquidity_atomics: Uint256 = total_liquidity.atomics();

    let liquidity_amount = if let Some(amount) = amount {
        amount
    } else {
        total_liquidity_atomics
    };

    // We need to execute withdraw on the user's behalf
    let msg = authz.execute(
        &env.contract.address,
        MsgWithdrawPosition {
            position_id: carrot_position.id,
            sender: user.to_string(),
            liquidity_amount: liquidity_amount.to_string(),
        },
    );

    let asset0_osmosis = carrot_position.position.asset0.unwrap();
    let asset1_osmosis = carrot_position.position.asset1.unwrap();

    let withdrawn_funds = [
        Coin {
            denom: asset0_osmosis.denom,
            amount: Uint128::try_from(
                Uint256::from_str(&asset0_osmosis.amount)? * liquidity_amount
                    / total_liquidity_atomics,
            )?,
        },
        Coin {
            denom: asset1_osmosis.denom,
            amount: Uint128::try_from(
                Uint256::from_str(&asset1_osmosis.amount)? * liquidity_amount
                    / total_liquidity_atomics,
            )?,
        },
    ];

    Ok((
        msg,
        liquidity_amount,
        total_liquidity_atomics,
        withdrawn_funds,
    ))
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
    let (swap_msgs, assets_for_position) = swap_to_enter_position(
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
    let create_msg = app.auth_z(deps, Some(sender.clone()))?.execute(
        &env.contract.address,
        MsgCreatePosition {
            pool_id: config.pool_config.pool_id,
            sender: sender.to_string(),
            lower_tick,
            upper_tick,
            tokens_provided: assets_for_position.into(),
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
    user: Addr,
    config: Config,
) -> AppResult<Vec<CosmosMsg>> {
    let rewards_config = config.autocompound_rewards_config;

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
            None,
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
        .auth_z(deps, Some(user))?
        .execute(&env.contract.address, msg_send);
    rewards_messages.push(send_reward_to_contract_msg);

    let send_reward_to_executor_msg = reward_asset.transfer_msg(executor)?;

    rewards_messages.push(send_reward_to_executor_msg);

    Ok(rewards_messages)
}
