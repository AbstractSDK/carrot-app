use crate::{
    contract::{App, AppResult},
    error::AppError,
    helpers::{add_funds, get_proxy_balance},
    msg::{AppExecuteMsg, ExecuteMsg},
    replies::REPLY_AFTER_SWAPS_STEP,
    state::{CONFIG, TEMP_CURRENT_COIN, TEMP_DEPOSIT_COINS, TEMP_EXPECTED_SWAP_COIN},
    yield_sources::{yield_type::YieldType, DepositStep, OneDepositStrategy},
};
use abstract_app::{abstract_sdk::features::AbstractResponse, objects::AnsAsset};
use abstract_dex_adapter::DexInterface;
use abstract_sdk::features::AbstractNameService;
use cosmwasm_std::{wasm_execute, Coin, DepsMut, Env, MessageInfo, SubMsg, Uint128};
use cw_asset::AssetInfo;

use super::query::query_exchange_rate;

pub fn deposit_one_strategy(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    strategy: OneDepositStrategy,
    yield_type: YieldType,
    app: App,
) -> AppResult {
    if info.sender != env.contract.address {
        return Err(AppError::Unauthorized {});
    }

    TEMP_DEPOSIT_COINS.save(deps.storage, &vec![])?;

    // We go through all deposit steps.
    // If the step is a swap, we execute with a reply to catch the amount change and get the exact deposit amount
    let msg = strategy
        .0
        .into_iter()
        .map(|s| {
            s.into_iter()
                .map(|step| match step {
                    DepositStep::Swap {
                        asset_in,
                        denom_out,
                        expected_amount,
                    } => wasm_execute(
                        env.contract.address.clone(),
                        &ExecuteMsg::Module(AppExecuteMsg::ExecuteOneDepositSwapStep {
                            asset_in,
                            denom_out,
                            expected_amount,
                        }),
                        vec![],
                    )
                    .map(|msg| Some(SubMsg::reply_on_success(msg, REPLY_AFTER_SWAPS_STEP))),

                    DepositStep::UseFunds { asset } => {
                        TEMP_DEPOSIT_COINS.update(deps.storage, |funds| add_funds(funds, asset))?;
                        Ok(None)
                    }
                })
                .collect::<Result<Vec<Option<SubMsg>>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;

    let msgs = msg.into_iter().flatten().flatten().collect::<Vec<_>>();

    // Finalize and execute the deposit
    let last_step = wasm_execute(
        env.contract.address.clone(),
        &ExecuteMsg::Module(AppExecuteMsg::FinalizeDeposit { yield_type }),
        vec![],
    )?;

    Ok(app
        .response("deposit-one")
        .add_submessages(msgs)
        .add_message(last_step))
}

pub fn execute_one_deposit_step(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    asset_in: Coin,
    denom_out: String,
    expected_amount: Uint128,
    app: App,
) -> AppResult {
    if info.sender != env.contract.address {
        return Err(AppError::Unauthorized {});
    }

    let config = CONFIG.load(deps.storage)?;

    let exchange_rate_in = query_exchange_rate(deps.as_ref(), asset_in.denom.clone(), &app)?;
    let exchange_rate_out = query_exchange_rate(deps.as_ref(), denom_out.clone(), &app)?;

    let ans = app.name_service(deps.as_ref());

    let asset_entries = ans.query(&vec![
        AssetInfo::native(asset_in.denom.clone()),
        AssetInfo::native(denom_out.clone()),
    ])?;
    let in_asset = asset_entries[0].clone();
    let out_asset = asset_entries[1].clone();

    let msg = app.ans_dex(deps.as_ref(), config.dex).swap(
        AnsAsset::new(in_asset, asset_in.amount),
        out_asset,
        None,
        Some(exchange_rate_in / exchange_rate_out),
    )?;

    let proxy_balance_before = get_proxy_balance(deps.as_ref(), &app, denom_out)?;
    TEMP_CURRENT_COIN.save(deps.storage, &proxy_balance_before)?;
    TEMP_EXPECTED_SWAP_COIN.save(deps.storage, &expected_amount)?;

    Ok(app.response("one-deposit-step").add_message(msg))
}

pub fn execute_finalize_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    yield_type: YieldType,
    app: App,
) -> AppResult {
    if info.sender != env.contract.address {
        return Err(AppError::Unauthorized {});
    }
    let available_deposit_coins = TEMP_DEPOSIT_COINS.load(deps.storage)?;

    let msgs = yield_type.deposit(deps.as_ref(), &env, available_deposit_coins, &app)?;

    Ok(app.response("one-deposit-step").add_submessages(msgs))
}
