use crate::{
    autocompound::AutocompoundState,
    check::Checkable,
    contract::{App, AppResult},
    distribution::deposit::generate_deposit_strategy,
    error::AppError,
    handlers::query::query_balance,
    helpers::assert_contract,
    msg::{AppExecuteMsg, ExecuteMsg},
    state::{AUTOCOMPOUND_STATE, CONFIG, STRATEGY_CONFIG},
    yield_sources::{AssetShare, StrategyUnchecked},
};
use abstract_app::abstract_sdk::features::AbstractResponse;
use abstract_sdk::ExecutorMsg;
use cosmwasm_std::{
    to_json_binary, Coin, Coins, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Uint128,
    WasmMsg,
};
use super::internal::execute_internal_action;

pub fn execute_handler(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    app: App,
    msg: AppExecuteMsg,
) -> AppResult {
    match msg {
        AppExecuteMsg::Deposit {
            funds,
            yield_sources_params,
        } => deposit(deps, env, info, funds, yield_sources_params, app),
        AppExecuteMsg::Withdraw { amount } => withdraw(deps, env, info, amount, app),
        AppExecuteMsg::Autocompound {} => autocompound(deps, env, info, app),
        AppExecuteMsg::UpdateStrategy { strategy, funds } => {
            update_strategy(deps, env, info, strategy, funds, app)
        }
        // Endpoints called by the contract directly
        AppExecuteMsg::Internal(internal_msg) => {
            assert_contract(&info, &env)?;
            execute_internal_action(deps, env, internal_msg, app)
        }
    }
}

fn deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    funds: Vec<Coin>,
    yield_source_params: Option<Vec<Option<Vec<AssetShare>>>>,
    app: App,
) -> AppResult {
    // Only the admin (manager contracts or account owner) can deposit as well as the contract itself
    app.admin
        .assert_admin(deps.as_ref(), &info.sender)
        .or(assert_contract(&info, &env))?;

    let deposit_msgs = _inner_deposit(deps.as_ref(), &env, funds, yield_source_params, &app)?;

    AUTOCOMPOUND_STATE.save(
        deps.storage,
        &AutocompoundState {
            last_compound: env.block.time,
        },
    )?;

    Ok(app.response("deposit").add_messages(deposit_msgs))
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

    let msgs = _inner_withdraw(deps, &env, amount, &app)?;

    Ok(app.response("withdraw").add_messages(msgs))
}

fn update_strategy(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    strategy: StrategyUnchecked,
    funds: Vec<Coin>,
    app: App,
) -> AppResult {
    // We load it raw because we're changing the strategy
    let old_strategy = STRATEGY_CONFIG.load(deps.storage)?;

    // We check the new strategy
    let strategy = strategy.check(deps.as_ref(), &app)?;

    // We execute operations to rebalance the funds between the strategies
    let mut available_funds: Coins = funds.try_into()?;
    // 1. We withdraw all yield_sources that are not included in the new strategies
    let all_stale_sources: Vec<_> = old_strategy
        .0
        .into_iter()
        .filter(|x| !strategy.0.contains(x))
        .collect();

    let (withdrawn_funds, withdraw_msgs): (Vec<Vec<Coin>>, Vec<Option<ExecutorMsg>>) =
        all_stale_sources
            .into_iter()
            .map(|s| {
                Ok::<_, AppError>((
                    s.withdraw_preview(deps.as_ref(), None, &app)
                        .unwrap_or_default(),
                    s.withdraw(deps.as_ref(), None, &app).ok(),
                ))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();

    withdrawn_funds
        .into_iter()
        .try_for_each(|f| f.into_iter().try_for_each(|f| available_funds.add(f)))?;

    // 2. We replace the strategy with the new strategy
    STRATEGY_CONFIG.save(deps.storage, &strategy)?;

    // 3. We deposit the funds into the new strategy
    let deposit_msgs = _inner_deposit(deps.as_ref(), &env, available_funds.into(), None, &app)?;

    Ok(app
        .response("rebalance")
        .add_messages(
            withdraw_msgs
                .into_iter()
                .flatten()
                .collect::<Vec<ExecutorMsg>>(),
        )
        .add_messages(deposit_msgs))
}

// /// Auto-compound the position with earned fees and incentives.

fn autocompound(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Everyone can autocompound
    let strategy = STRATEGY_CONFIG.load(deps.storage)?;

    // We withdraw all rewards from protocols
    let (all_rewards, collect_rewards_msgs) = strategy.withdraw_rewards(deps.as_ref(), &app)?;

    // If there are no rewards, we can't do anything
    if all_rewards.is_empty() {
        return Err(crate::error::AppError::NoRewards {});
    }

    // Finally we deposit of all rewarded tokens into the position
    let msg_deposit = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::Deposit {
            funds: all_rewards,
            yield_sources_params: None,
        }))?,
        funds: vec![],
    });

    let response = app
        .response("auto-compound")
        .add_messages(collect_rewards_msgs)
        .add_message(msg_deposit);

    let config = CONFIG.load(deps.storage)?;
    let executor_reward_messages =
        config.get_executor_reward_messages(deps.as_ref(), &env, info, &app)?;

    AUTOCOMPOUND_STATE.save(
        deps.storage,
        &AutocompoundState {
            last_compound: env.block.time,
        },
    )?;

    Ok(response.add_messages(executor_reward_messages))
}

// /// UNUSED FOR NOW, replaces by _inner_advanced_deposit
// /// The deposit process goes through the following steps
// /// 1. We query the target strategy in storage
// /// 2. We correct the expected token shares of each strategy, in case there are corrections passed to the function
// /// 3. We deposit funds according to that strategy
// ///
// /// This approach is not perfect. TO show the flaws, take an example where you allocate 50% into mars, 50% into osmosis and both give similar rewards.
// /// Assume we deposited 2x inside the app.
// /// When an auto-compounding happens, they both get y as rewards, mars is already auto-compounding and osmosis' rewards are redeposited inside the pool
// /// Step | Mars | Osmosis | Rewards|
// /// Deposit | x | x | 0 |
// /// Withdraw Rewards | x + y | x| y |
// /// Re-deposit | x + y + y/2 | x + y/2 | 0 |
// /// The final ratio is not the 50/50 ratio we target
// ///
// /// PROPOSITION : We could also have this kind of deposit flow
// /// 1a. We query the target strategy in storage (target strategy)
// /// 1b. We query the current status of the strategy (current strategy)
// /// 1c. We create a temporary strategy object to allocate the funds from this deposit into the various strategies
// /// --> the goal of those 3 steps is to correct the funds allocation faster towards the target strategy
// /// 2. We correct the expected token shares of each strategy, in case there are corrections passed to the function
// /// 3. We deposit funds according to that strategy
// /// This time :
// /// Step | Mars | Osmosis | Rewards|
// /// Deposit | x | x | 0 |
// /// Withdraw Rewards | x + y | x| y |
// /// Re-deposit | x + y | x + y | 0 |
// pub fn _inner_deposit(
//     deps: Deps,
//     env: &Env,
//     funds: Vec<Coin>,
//     yield_source_params: Option<Vec<Option<Vec<AssetShare>>>>,
//     app: &App,
// ) -> AppResult<Vec<CosmosMsg>> {
//     // We query the target strategy depending on the existing deposits
//     let mut current_strategy_status = CONFIG.load(deps.storage)?.strategy;
//     current_strategy_status.apply_current_strategy_shares(deps, app)?;

//     // We correct it if the user asked to correct the share parameters of each strategy
//     current_strategy_status.correct_with(yield_source_params);

//     // We fill the strategies with the current deposited funds and get messages to execute those deposits
//     current_strategy_status.fill_all_and_get_messages(deps, env, funds, app)
// }

pub fn _inner_deposit(
    deps: Deps,
    env: &Env,
    funds: Vec<Coin>,
    yield_source_params: Option<Vec<Option<Vec<AssetShare>>>>,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
    let (withdraw_strategy, deposit_msgs) =
        generate_deposit_strategy(deps, funds, yield_source_params, app)?;
    let deposit_withdraw_msgs = withdraw_strategy
        .into_iter()
        .map(|(el, share)| el.withdraw(deps, Some(share), app).map(Into::into))
        .collect::<Result<Vec<_>, _>>()?;
    let deposit_msgs = deposit_msgs
        .into_iter()
        .map(|msg| msg.to_cosmos_msg(env))
        .collect::<Result<Vec<_>, _>>()?;

    Ok([deposit_withdraw_msgs, deposit_msgs].concat())
}

fn _inner_withdraw(
    deps: DepsMut,
    _env: &Env,
    value: Option<Uint128>,
    app: &App,
) -> AppResult<Vec<ExecutorMsg>> {
    // We need to select the share of each investment that needs to be withdrawn
    let withdraw_share = value
        .map(|value| {
            let total_deposit = query_balance(deps.as_ref(), app)?;

            if total_deposit.total_value.is_zero() {
                return Err(AppError::NoDeposit {});
            }
            Ok(Decimal::from_ratio(value, total_deposit.total_value))
        })
        .transpose()?;

    // We withdraw the necessary share from all registered investments
    let withdraw_msgs =
        STRATEGY_CONFIG
            .load(deps.storage)?
            .withdraw(deps.as_ref(), withdraw_share, app)?;

    Ok(withdraw_msgs.into_iter().collect())
}
