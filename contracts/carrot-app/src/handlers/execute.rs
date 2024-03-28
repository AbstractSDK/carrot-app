use crate::{
    contract::{App, AppResult},
    error::AppError,
    handlers::query::query_balance,
    msg::{AppExecuteMsg, ExecuteMsg},
    state::{assert_contract, get_autocompound_status, Config, CONFIG},
    yield_sources::BalanceStrategy,
};
use abstract_app::{abstract_sdk::features::AbstractResponse, objects::AnsAsset};
use abstract_dex_adapter::DexInterface;
use abstract_sdk::{AccountAction, Execution, ExecutorMsg, TransferInterface};
use cosmwasm_std::{
    to_json_binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Uint128, WasmMsg,
};

use super::{
    internal::{deposit_one_strategy, execute_finalize_deposit, execute_one_deposit_step},
    query::{query_all_exchange_rates, query_exchange_rate, query_strategy},
    swap_helpers::swap_msg,
};

pub fn execute_handler(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    app: App,
    msg: AppExecuteMsg,
) -> AppResult {
    match msg {
        AppExecuteMsg::Deposit { funds } => deposit(deps, env, info, funds, app),
        AppExecuteMsg::Withdraw { amount } => withdraw(deps, env, info, amount, app),
        AppExecuteMsg::Autocompound {} => autocompound(deps, env, info, app),
        AppExecuteMsg::Rebalance { strategy } => rebalance(deps, env, info, strategy, app),

        // Endpoints called by the contract directly
        AppExecuteMsg::DepositOneStrategy {
            swap_strategy,
            yield_type,
            yield_index,
        } => deposit_one_strategy(deps, env, info, swap_strategy, yield_index, yield_type, app),
        AppExecuteMsg::ExecuteOneDepositSwapStep {
            asset_in,
            denom_out,
            expected_amount,
        } => execute_one_deposit_step(deps, env, info, asset_in, denom_out, expected_amount, app),
        AppExecuteMsg::FinalizeDeposit {
            yield_type,
            yield_index,
        } => execute_finalize_deposit(deps, env, info, yield_type, yield_index, app),
    }
}

fn deposit(deps: DepsMut, env: Env, info: MessageInfo, funds: Vec<Coin>, app: App) -> AppResult {
    // Only the admin (manager contracts or account owner) can deposit as well as the contract itself
    app.admin
        .assert_admin(deps.as_ref(), &info.sender)
        .or(assert_contract(&info, &env))?;

    let deposit_msgs = _inner_deposit(deps.as_ref(), &env, funds, &app)?;
    deps.api
        .debug(&format!("All deposit messages {:?}", deposit_msgs));

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

fn rebalance(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    strategy: BalanceStrategy,
    app: App,
) -> AppResult {
    // We load it raw because we're changing the strategy
    let mut config = CONFIG.load(deps.storage)?;
    let old_strategy = config.balance_strategy;
    strategy.check()?;

    // We execute operations to rebalance the funds between the strategies
    // TODO
    config.balance_strategy = strategy;
    CONFIG.save(deps.storage, &config)?;

    Ok(app.response("rebalance"))
}

// /// Auto-compound the position with earned fees and incentives.

fn autocompound(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Everyone can autocompound

    // We withdraw all rewards from protocols
    let (rewards, collect_rewards_msgs): (Vec<Vec<Coin>>, Vec<ExecutorMsg>) =
        query_strategy(deps.as_ref())?
            .strategy
            .0
            .into_iter()
            .map(|s| {
                let (rewards, raw_msgs) =
                    s.yield_source.ty.withdraw_rewards(deps.as_ref(), &app)?;

                Ok::<_, AppError>((
                    rewards,
                    app.executor(deps.as_ref())
                        .execute(vec![AccountAction::from_vec(raw_msgs)])?,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();

    let all_rewards: Vec<Coin> = rewards.into_iter().flatten().collect();
    // If there are no rewards, we can't do anything
    if all_rewards.is_empty() {
        return Err(crate::error::AppError::NoRewards {});
    }

    // Finally we deposit of all rewarded tokens into the position
    let msg_deposit = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::Deposit {
            funds: all_rewards,
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
        && get_autocompound_status(
            deps.storage,
            &env,
            config.autocompound_config.cooldown_seconds.u64(),
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

pub fn _inner_deposit(
    deps: Deps,
    env: &Env,
    funds: Vec<Coin>,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
    let strategy = query_strategy(deps)?.strategy;

    // We determine the value of all tokens that will be used inside this function
    let exchange_rates = query_all_exchange_rates(
        deps,
            strategy
            .0.clone()
            .into_iter()
            .flat_map(|s| {
                s.yield_source
                    .asset_distribution
                    .into_iter()
                    .map(|(denom, _)| denom)
            })
            .chain(funds.iter().map(|f| f.denom.clone())),
        app,
    )?;

    let deposit_strategies = strategy
        .fill_all(funds, &exchange_rates)?;

    // We select the target shares depending on the strategy selected
    let deposit_msgs = deposit_strategies
        .iter()
        .zip(
            strategy
                .0
                .iter()
                .map(|s| s.yield_source.ty.clone()),
        )
        .enumerate()
        .map(|(index, (strategy, yield_type))| strategy.deposit_msgs(env, index, yield_type))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(deposit_msgs)
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
            let total_value = total_deposit
                .balances
                .into_iter()
                .map(|balance| {
                    let exchange_rate = query_exchange_rate(deps.as_ref(), balance.denom, app)?;

                    Ok::<_, AppError>(exchange_rate * balance.amount)
                })
                .sum::<Result<Uint128, _>>()?;

            if total_value.is_zero() {
                return Err(AppError::NoDeposit {});
            }
            Ok(Decimal::from_ratio(value, total_value))
        })
        .transpose()?;

    // We withdraw the necessary share from all investments
    let withdraw_msgs = query_strategy(deps.as_ref())?
        .strategy
        .0
        .into_iter()
        .map(|s| {
            let this_withdraw_amount = withdraw_share
                .map(|share| {
                    let this_amount = s.yield_source.ty.user_liquidity(deps.as_ref(), app)?;
                    let this_withdraw_amount = share * this_amount;

                    Ok::<_, AppError>(this_withdraw_amount)
                })
                .transpose()?;
            let raw_msg = s
                .yield_source
                .ty
                .withdraw(deps.as_ref(), this_withdraw_amount, app)?;

            Ok::<_, AppError>(
                app.executor(deps.as_ref())
                    .execute(vec![AccountAction::from_vec(raw_msg)])?,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(withdraw_msgs.into_iter().collect())
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
    let rewards_config = config.autocompound_config.rewards;

    // Get user balance of gas denom
    let user_gas_balance = app.bank(deps).balance(&rewards_config.gas_asset)?.amount;

    let mut rewards_messages = vec![];

    // If not enough gas coins - swap for some amount
    if user_gas_balance < rewards_config.min_gas_balance {
        // Get asset entries
        let dex = app.ans_dex(deps, config.dex.to_string());

        // Do reverse swap to find approximate amount we need to swap
        let need_gas_coins = rewards_config.max_gas_balance - user_gas_balance;
        let simulate_swap_response = dex.simulate_swap(
            AnsAsset::new(rewards_config.gas_asset.clone(), need_gas_coins),
            rewards_config.swap_asset.clone(),
        )?;

        // Get user balance of swap denom
        let user_swap_balance = app.bank(deps).balance(&rewards_config.swap_asset)?.amount;

        // Swap as much as available if not enough for max_gas_balance
        let swap_amount = simulate_swap_response.return_amount.min(user_swap_balance);

        let msgs = swap_msg(
            deps,
            env,
            AnsAsset::new(rewards_config.swap_asset, swap_amount),
            rewards_config.gas_asset.clone(),
            app,
        )?;
        rewards_messages.extend(msgs);
    }

    // We send their reward to the executor
    let msg_send = app.bank(deps).transfer(
        vec![AnsAsset::new(
            rewards_config.gas_asset,
            rewards_config.reward,
        )],
        &deps.api.addr_validate(&executor)?,
    )?;

    rewards_messages.push(app.executor(deps).execute(vec![msg_send])?.into());

    Ok(rewards_messages)
}
