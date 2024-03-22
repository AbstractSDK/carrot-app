use crate::{
    contract::{App, AppResult},
    error::AppError,
    handlers::query::query_balance,
    helpers::{add_funds, get_balance, get_proxy_balance},
    msg::{AppExecuteMsg, ExecuteMsg},
    replies::REPLY_AFTER_SWAPS_STEP,
    state::{
        assert_contract, Config, CONFIG, TEMP_CURRENT_COIN, TEMP_DEPOSIT_COINS,
        TEMP_EXPECTED_SWAP_COIN,
    },
    yield_sources::{yield_type::YieldType, DepositStep, OneDepositStrategy},
};
use abstract_app::{abstract_sdk::features::AbstractResponse, objects::AnsAsset};
use abstract_dex_adapter::DexInterface;
use abstract_sdk::features::AbstractNameService;
use cosmwasm_std::{
    to_json_binary, wasm_execute, Coin, Coins, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo,
    StdError, SubMsg, Uint128, WasmMsg,
};
use cw_asset::{Asset, AssetInfo};
use osmosis_std::{
    cosmwasm_to_proto_coins, try_proto_to_cosmwasm_coins,
    types::osmosis::concentratedliquidity::v1beta1::{
        MsgAddToPosition, MsgCollectIncentives, MsgCollectSpreadRewards, MsgCreatePosition,
        MsgWithdrawPosition,
    },
};
use std::{collections::HashMap, str::FromStr};

use super::{
    internal::{deposit_one_strategy, execute_finalize_deposit, execute_one_deposit_step},
    query::{query_exchange_rate, query_strategy},
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
        AppExecuteMsg::Autocompound {} => todo!(),
        AppExecuteMsg::Rebalance { strategy } => todo!(),

        // Endpoints called by the contract directly
        AppExecuteMsg::DepositOneStrategy {
            swap_strategy,
            yield_type,
        } => deposit_one_strategy(deps, env, info, swap_strategy, yield_type, app),
        AppExecuteMsg::ExecuteOneDepositSwapStep {
            asset_in,
            denom_out,
            expected_amount,
        } => execute_one_deposit_step(deps, env, info, asset_in, denom_out, expected_amount, app),
        AppExecuteMsg::FinalizeDeposit { yield_type } => {
            execute_finalize_deposit(deps, env, info, yield_type, app)
        }
    }
}

fn deposit(deps: DepsMut, env: Env, info: MessageInfo, funds: Vec<Coin>, app: App) -> AppResult {
    // Only the admin (manager contracts or account owner) can deposit
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;

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

// /// Auto-compound the position with earned fees and incentives.

// fn autocompound(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
//     // Everyone can autocompound

//     let position = get_osmosis_position(deps.as_ref())?;
//     let position_details = position.position.unwrap();

//     let mut rewards = cosmwasm_std::Coins::default();
//     let mut collect_rewards_msgs = vec![];

//     // Get app's user and set up authz.
//     let user = get_user(deps.as_ref(), &app)?;
//     let authz = app.auth_z(deps.as_ref(), Some(user.clone()))?;

//     // If there are external incentives, claim them.
//     if !position.claimable_incentives.is_empty() {
//         for coin in try_proto_to_cosmwasm_coins(position.claimable_incentives)? {
//             rewards.add(coin)?;
//         }
//         collect_rewards_msgs.push(authz.execute(
//             &env.contract.address,
//             MsgCollectIncentives {
//                 position_ids: vec![position_details.position_id],
//                 sender: user.to_string(),
//             },
//         ));
//     }

//     // If there is income from swap fees, claim them.
//     if !position.claimable_spread_rewards.is_empty() {
//         for coin in try_proto_to_cosmwasm_coins(position.claimable_spread_rewards)? {
//             rewards.add(coin)?;
//         }
//         collect_rewards_msgs.push(authz.execute(
//             &env.contract.address,
//             MsgCollectSpreadRewards {
//                 position_ids: vec![position_details.position_id],
//                 sender: position_details.address.clone(),
//             },
//         ))
//     }

//     // If there are no rewards, we can't do anything
//     if rewards.is_empty() {
//         return Err(crate::error::AppError::NoRewards {});
//     }

//     // Finally we deposit of all rewarded tokens into the position
//     let msg_deposit = CosmosMsg::Wasm(WasmMsg::Execute {
//         contract_addr: env.contract.address.to_string(),
//         msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::Deposit {
//             funds: rewards.into(),
//             max_spread: None,
//             belief_price0: None,
//             belief_price1: None,
//         }))?,
//         funds: vec![],
//     });

//     let mut response = app
//         .response("auto-compound")
//         .add_messages(collect_rewards_msgs)
//         .add_message(msg_deposit);

//     // If called by non-admin and reward cooldown has ended, send rewards to the contract caller.
//     let config = CONFIG.load(deps.storage)?;
//     if !app.admin.is_admin(deps.as_ref(), &info.sender)?
//         && get_position_status(
//             deps.storage,
//             &env,
//             config.autocompound_cooldown_seconds.u64(),
//         )?
//         .is_ready()
//     {
//         let executor_reward_messages = autocompound_executor_rewards(
//             deps.as_ref(),
//             &env,
//             info.sender.into_string(),
//             &app,
//             config,
//         )?;

//         response = response.add_messages(executor_reward_messages);
//     }

//     Ok(response)
// }

pub fn _inner_deposit(
    deps: Deps,
    env: &Env,
    funds: Vec<Coin>,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
    // We determine the value of all the tokens that were received with USD

    let all_strategy_exchange_rates = query_strategy(deps)?.strategy.0.into_iter().flat_map(|s| {
        s.yield_source
            .expected_tokens
            .into_iter()
            .map(|(denom, _)| {
                Ok::<_, AppError>((
                    denom.clone(),
                    query_exchange_rate(deps, denom.clone(), app)?,
                ))
            })
    });
    let exchange_rates = funds
        .iter()
        .map(|f| {
            Ok::<_, AppError>((
                f.denom.clone(),
                query_exchange_rate(deps, f.denom.clone(), app)?,
            ))
        })
        .chain(all_strategy_exchange_rates)
        .collect::<Result<HashMap<_, _>, _>>()?;

    let deposit_strategies = query_strategy(deps)?
        .strategy
        .fill_all(funds, &exchange_rates)?;

    // We select the target shares depending on the strategy selected
    let deposit_msgs = deposit_strategies
        .iter()
        .zip(
            query_strategy(deps)?
                .strategy
                .0
                .iter()
                .map(|s| s.yield_source.ty.clone()),
        )
        .map(|(strategy, yield_type)| strategy.deposit_msgs(env, yield_type))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(deposit_msgs)
}

fn _inner_withdraw(
    deps: DepsMut,
    _env: &Env,
    value: Option<Uint128>,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
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
            s.yield_source
                .ty
                .withdraw(deps.as_ref(), this_withdraw_amount, app)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(withdraw_msgs.into_iter().flatten().collect())
}

// /// Sends autocompound rewards to the executor.
// /// In case user does not have not enough gas token the contract will swap some
// /// tokens for gas tokens.
// pub fn autocompound_executor_rewards(
//     deps: Deps,
//     env: &Env,
//     executor: String,
//     app: &App,
//     config: Config,
// ) -> AppResult<Vec<CosmosMsg>> {
//     let rewards_config = config.autocompound_rewards_config;
//     let position = get_position(deps)?;
//     let user = position.owner;

//     // Get user balance of gas denom
//     let gas_denom = rewards_config
//         .gas_asset
//         .resolve(&deps.querier, &app.ans_host(deps)?)?;
//     let user_gas_balance = gas_denom.query_balance(&deps.querier, user.clone())?;

//     let mut rewards_messages = vec![];

//     // If not enough gas coins - swap for some amount
//     if user_gas_balance < rewards_config.min_gas_balance {
//         // Get asset entries
//         let dex = app.ans_dex(deps, OSMOSIS.to_string());

//         // Do reverse swap to find approximate amount we need to swap
//         let need_gas_coins = rewards_config.max_gas_balance - user_gas_balance;
//         let simulate_swap_response = dex.simulate_swap(
//             AnsAsset::new(rewards_config.gas_asset.clone(), need_gas_coins),
//             rewards_config.swap_asset.clone(),
//         )?;

//         // Get user balance of swap denom
//         let user_swap_balance =
//             get_balance(rewards_config.swap_asset.clone(), deps, user.clone(), app)?;

//         // Swap as much as available if not enough for max_gas_balance
//         let swap_amount = simulate_swap_response.return_amount.min(user_swap_balance);

//         let msgs = swap_msg(
//             deps,
//             env,
//             AnsAsset::new(rewards_config.swap_asset, swap_amount),
//             rewards_config.gas_asset,
//             app,
//         )?;
//         rewards_messages.extend(msgs);
//     }

//     let reward_asset = Asset::new(gas_denom, rewards_config.reward);
//     let msg_send = reward_asset.transfer_msg(env.contract.address.to_string())?;

//     // To avoid giving general `MsgSend` authorization to any address we do 2 sends here
//     // 1) From user to the contract
//     // 2) From contract to the executor
//     // That way we can limit the `MsgSend` authorization to the contract address only.
//     let send_reward_to_contract_msg = app
//         .auth_z(deps, Some(cosmwasm_std::Addr::unchecked(user)))?
//         .execute(&env.contract.address, msg_send);
//     rewards_messages.push(send_reward_to_contract_msg);

//     let send_reward_to_executor_msg = reward_asset.transfer_msg(executor)?;

//     rewards_messages.push(send_reward_to_executor_msg);

//     Ok(rewards_messages)
// }
