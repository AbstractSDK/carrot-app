use crate::{
    contract::{App, AppResult},
    error::AppError,
    handlers::query::query_balance,
    helpers::assert_contract,
    msg::{AppExecuteMsg, ExecuteMsg, InternalExecuteMsg},
    state::{AUTOCOMPOUND_STATE, CONFIG},
    yield_sources::{AssetShare, BalanceStrategyUnchecked, Checkable},
};
use abstract_app::abstract_sdk::features::AbstractResponse;
use abstract_sdk::ExecutorMsg;
use cosmwasm_std::{
    to_json_binary, Coin, Coins, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Uint128,
    WasmMsg,
};

use super::internal::{deposit_one_strategy, execute_finalize_deposit, execute_one_deposit_step};
use abstract_app::traits::AccountIdentification;

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
    env: Env,
    _info: MessageInfo,
    strategy: BalanceStrategyUnchecked,
    funds: Vec<Coin>,
    app: App,
) -> AppResult {
    // We load it raw because we're changing the strategy
    let mut config = CONFIG.load(deps.storage)?;
    let old_strategy = config.balance_strategy;

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

    deps.api.debug("After stale sources");
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

    deps.api
        .debug(&format!("After withdraw messages : {:?}", withdrawn_funds));
    withdrawn_funds
        .into_iter()
        .try_for_each(|f| f.into_iter().try_for_each(|f| available_funds.add(f)))?;

    // 2. We replace the strategy with the new strategy
    config.balance_strategy = strategy;
    CONFIG.save(deps.storage, &config)?;

    // 3. We deposit the funds into the new strategy
    let deposit_msgs = _inner_deposit(deps.as_ref(), &env, available_funds.into(), None, &app)?;

    deps.api.debug(&format!(
        "Proxy balance before withdraw : {:?}",
        deps.querier
            .query_all_balances(app.account_base(deps.as_ref())?.proxy)?
    ));

    deps.api.debug("After deposit msgs");
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
    let strategy = CONFIG.load(deps.storage)?.balance_strategy;

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
    let mut current_strategy_status = CONFIG.load(deps.storage)?.balance_strategy;
    current_strategy_status.apply_current_strategy_shares(deps, app)?;

    // We correct it if the user asked to correct the share parameters of each strategy
    current_strategy_status.correct_with(yield_source_params);

    // We fill the strategies with the current deposited funds and get messages to execute those deposits
    current_strategy_status.fill_all_and_get_messages(deps, env, funds, app)
}

pub fn _inner_advanced_deposit(
    deps: Deps,
    env: &Env,
    funds: Vec<Coin>,
    yield_source_params: Option<Vec<Option<Vec<AssetShare>>>>,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
    // This is the storage strategy for all assets
    let target_strategy = CONFIG.load(deps.storage)?.balance_strategy;

    // This is the current distribution of funds inside the strategies
    let current_strategy_status = target_strategy.query_current_status(deps, app)?;

    let mut usable_funds: Coins = funds.try_into()?;
    let (withdraw_msgs, this_deposit_strategy) = target_strategy.current_deposit_strategy(
        deps,
        &mut usable_funds,
        current_strategy_status,
        app,
    )?;

    let mut this_deposit_strategy = if let Some(this_deposit_strategy) = this_deposit_strategy {
        this_deposit_strategy
    } else {
        return Ok(withdraw_msgs);
    };

    // We query the yield source shares
    this_deposit_strategy.apply_current_strategy_shares(deps, app)?;

    // We correct it if the user asked to correct the share parameters of each strategy
    this_deposit_strategy.correct_with(yield_source_params);

    // We fill the strategies with the current deposited funds and get messages to execute those deposits
    let deposit_msgs =
        this_deposit_strategy.fill_all_and_get_messages(deps, env, usable_funds.into(), app)?;

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
        CONFIG
            .load(deps.storage)?
            .balance_strategy
            .withdraw(deps.as_ref(), withdraw_share, app)?;

    Ok(withdraw_msgs.into_iter().collect())
}
