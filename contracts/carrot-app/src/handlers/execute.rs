use crate::{
    contract::{App, AppResult},
    error::AppError,
    handlers::query::query_balance,
    helpers::{assert_contract, compute_value},
    msg::{AppExecuteMsg, ExecuteMsg, InternalExecuteMsg},
    state::{AUTOCOMPOUND_STATE, CONFIG},
    yield_sources::{AssetShare, BalanceStrategy, BalanceStrategyElement},
};
use abstract_app::abstract_sdk::features::AbstractResponse;
use abstract_sdk::ExecutorMsg;
use cosmwasm_std::{
    to_json_binary, Coin, Coins, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Uint128,
    WasmMsg,
};

use super::{
    internal::{deposit_one_strategy, execute_finalize_deposit, execute_one_deposit_step},
    query::query_strategy,
};

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
        AppExecuteMsg::UpdateStrategy { strategy } => {
            update_strategy(deps, env, info, strategy, app)
        }
        // Endpoints called by the contract directly
        AppExecuteMsg::Internal(internal_msg) => {
            if info.sender != env.contract.address {
                return Err(AppError::Unauthorized {});
            }
            match internal_msg {
                InternalExecuteMsg::DepositOneStrategy {
                    swap_strategy,
                    yield_type,
                    yield_index,
                } => deposit_one_strategy(deps, env, swap_strategy, yield_index, yield_type, app),
                InternalExecuteMsg::ExecuteOneDepositSwapStep {
                    asset_in,
                    denom_out,
                    expected_amount,
                } => execute_one_deposit_step(deps, env, asset_in, denom_out, expected_amount, app),
                InternalExecuteMsg::FinalizeDeposit {
                    yield_type,
                    yield_index,
                } => execute_finalize_deposit(deps, yield_type, yield_index, app),
            }
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

    // let deposit_msgs = _inner_deposit(deps.as_ref(), &env, funds, yield_source_params, &app)?;
    let deposit_msgs =
        _inner_advanced_deposit(deps.as_ref(), &env, funds, yield_source_params, &app)?;

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
    _env: Env,
    _info: MessageInfo,
    strategy: BalanceStrategy,
    app: App,
) -> AppResult {
    // We load it raw because we're changing the strategy
    let mut config = CONFIG.load(deps.storage)?;
    let old_strategy = config.balance_strategy;

    strategy.check(deps.as_ref(), &app)?;

    // We execute operations to rebalance the funds between the strategies
    // TODO
    config.balance_strategy = strategy;
    CONFIG.save(deps.storage, &config)?;

    Ok(app.response("rebalance"))
}

// /// Auto-compound the position with earned fees and incentives.

fn autocompound(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Everyone can autocompound
    let strategy = query_strategy(deps.as_ref())?.strategy;

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

    AUTOCOMPOUND_STATE.update(deps.storage, |mut state| {
        state.last_compound = env.block.time;
        Ok::<_, AppError>(state)
    })?;

    Ok(response.add_messages(executor_reward_messages))
}

/// The deposit process goes through the following steps
/// 1. We query the target strategy in storage
/// 2. We correct the expected token shares of each strategy, in case there are corrections passed to the function
/// 3. We deposit funds according to that strategy
///
/// This approach is not perfect. TO show the flaws, take an example where you allocate 50% into mars, 50% into osmosis and both give similar rewards.
/// Assume we deposited 2x inside the app.
/// When an auto-compounding happens, they both get y as rewards, mars is already auto-compounding and osmosis' rewards are redeposited inside the pool
/// Step | Mars | Osmosis | Rewards|
/// Deposit | x | x | 0 |
/// Withdraw Rewards | x + y | x| y |
/// Re-deposit | x + y + y/2 | x + y/2 | 0 |
/// The final ratio is not the 50/50 ratio we target
///
/// PROPOSITION : We could also have this kind of deposit flow
/// 1a. We query the target strategy in storage (target strategy)
/// 1b. We query the current status of the strategy (current strategy)
/// 1c. We create a temporary strategy object to allocate the funds from this deposit into the various strategies
/// --> the goal of those 3 steps is to correct the funds allocation faster towards the target strategy
/// 2. We correct the expected token shares of each strategy, in case there are corrections passed to the function
/// 3. We deposit funds according to that strategy
/// This time :
/// Step | Mars | Osmosis | Rewards|
/// Deposit | x | x | 0 |
/// Withdraw Rewards | x + y | x| y |
/// Re-deposit | x + y | x + y | 0 |
pub fn _inner_deposit(
    deps: Deps,
    env: &Env,
    funds: Vec<Coin>,
    yield_source_params: Option<Vec<Option<Vec<AssetShare>>>>,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
    // We query the target strategy depending on the existing deposits
    let mut current_strategy_status = query_strategy(deps)?.strategy;
    current_strategy_status.apply_current_strategy_shares(deps, app)?;

    // We correct it if the user asked to correct the share parameters of each strategy
    current_strategy_status.correct_with(yield_source_params);

    // We fill the strategies with the current deposited funds and get messages to execute those deposits
    current_strategy_status.fill_all_and_get_messages(deps, env, funds, app)
}

pub fn _inner_advanced_deposit(
    deps: Deps,
    env: &Env,
    mut funds: Vec<Coin>,
    yield_source_params: Option<Vec<Option<Vec<AssetShare>>>>,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
    // This is the storage strategy for all assets
    let target_strategy = query_strategy(deps)?.strategy;

    // This is the current distribution of funds inside the strategies
    let current_distribution = target_strategy.query_current_status(deps, app)?;
    let total_value = target_strategy.current_balance(deps, app)?.total_value;
    let deposit_value = compute_value(deps, &funds, app)?;

    if deposit_value.is_zero() {
        // We are trying to deposit no value, so we just don't do anything
        return Ok(vec![]);
    }

    // We create the strategy so that he final distribution is as close to the target strategy as possible
    // 1. For all strategies, we withdraw some if its value is too high above target_strategy
    let mut withdraw_funds = Coins::default();
    let mut withdraw_value = Uint128::zero();
    let mut withdraw_msgs = vec![];

    // All strategies have to be reviewed
    // EITHER of those are true :
    // - The yield source has too much funds deposited and some should be withdrawn
    // OR
    // - Some funds need to be deposited into the strategy
    let mut this_deposit_strategy: BalanceStrategy = target_strategy
        .0
        .iter()
        .zip(current_distribution.0)
        .map(|(target, current)| {
            // We need to take into account the total value added by the current shares

            let value_now = current.share * total_value;
            let target_value = target.share * (total_value + deposit_value);

            // If value now is greater than the target value, we need to withdraw some funds from the protocol
            if target_value < value_now {
                let this_withdraw_value = target_value - value_now;
                // In the following line, total_value can't be zero, otherwise the if condition wouldn't be met
                let this_withdraw_share = Decimal::from_ratio(withdraw_value, total_value);
                let this_withdraw_funds =
                    current.withdraw_preview(deps, Some(this_withdraw_share), app)?;
                withdraw_value += this_withdraw_value;
                for fund in this_withdraw_funds {
                    withdraw_funds.add(fund)?;
                }
                withdraw_msgs.push(
                    current
                        .withdraw(deps, Some(this_withdraw_share), app)?
                        .into(),
                );

                // In case there is a withdraw from the strategy, we don't need to deposit into this strategy after !
                Ok::<_, AppError>(None)
            } else {
                // In case we don't withdraw anything, it means we might deposit.
                // Total should sum to one !
                let share = Decimal::from_ratio(target_value - value_now, deposit_value);

                Ok(Some(BalanceStrategyElement {
                    yield_source: target.yield_source.clone(),
                    share,
                }))
            }
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .into();

    // We add the withdrawn funds to the deposit funds
    funds.extend(withdraw_funds);

    // We query the yield source shares
    this_deposit_strategy.apply_current_strategy_shares(deps, app)?;

    // We correct it if the user asked to correct the share parameters of each strategy
    this_deposit_strategy.correct_with(yield_source_params);

    // We fill the strategies with the current deposited funds and get messages to execute those deposits
    let deposit_msgs = this_deposit_strategy.fill_all_and_get_messages(deps, env, funds, app)?;

    Ok([withdraw_msgs, deposit_msgs].concat())
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
        query_strategy(deps.as_ref())?
            .strategy
            .withdraw(deps.as_ref(), withdraw_share, app)?;

    Ok(withdraw_msgs.into_iter().collect())
}
