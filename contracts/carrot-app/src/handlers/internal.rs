use crate::{
    contract::{App, AppResult},
    distribution::deposit::{DepositStep, OneDepositStrategy},
    helpers::get_proxy_balance,
    msg::{AppExecuteMsg, ExecuteMsg, InternalExecuteMsg},
    replies::REPLY_AFTER_SWAPS_STEP,
    state::{
        CONFIG, STRATEGY_CONFIG, TEMP_CURRENT_COIN, TEMP_CURRENT_YIELD, TEMP_DEPOSIT_COINS,
        TEMP_EXPECTED_SWAP_COIN,
    },
    yield_sources::{yield_type::YieldType, Strategy},
};
use abstract_app::{abstract_sdk::features::AbstractResponse, objects::AnsAsset};
use abstract_dex_adapter::DexInterface;
use abstract_sdk::features::AbstractNameService;
use cosmwasm_std::{wasm_execute, Coin, Coins, DepsMut, Env, SubMsg, Uint128};
use cw_asset::AssetInfo;

use crate::exchange_rate::query_exchange_rate;

pub fn execute_internal_action(
    deps: DepsMut,
    env: Env,
    internal_msg: InternalExecuteMsg,
    app: App,
) -> AppResult {
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

fn deposit_one_strategy(
    deps: DepsMut,
    env: Env,
    strategy: OneDepositStrategy,
    yield_index: usize,
    yield_type: YieldType,
    app: App,
) -> AppResult {
    deps.api.debug("Start deposit one strategy");
    let mut temp_deposit_coins = Coins::default();

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
                        &ExecuteMsg::Module(AppExecuteMsg::Internal(
                            InternalExecuteMsg::ExecuteOneDepositSwapStep {
                                asset_in,
                                denom_out,
                                expected_amount,
                            },
                        )),
                        vec![],
                    )
                    .map(|msg| Some(SubMsg::reply_on_success(msg, REPLY_AFTER_SWAPS_STEP))),

                    DepositStep::UseFunds { asset } => {
                        temp_deposit_coins.add(asset)?;
                        Ok(None)
                    }
                })
                .collect::<Result<Vec<Option<SubMsg>>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;

    TEMP_DEPOSIT_COINS.save(deps.storage, &temp_deposit_coins.into())?;

    let msgs = msg.into_iter().flatten().flatten().collect::<Vec<_>>();

    // Finalize and execute the deposit
    let last_step = wasm_execute(
        env.contract.address.clone(),
        &ExecuteMsg::Module(AppExecuteMsg::Internal(
            InternalExecuteMsg::FinalizeDeposit {
                yield_type,
                yield_index,
            },
        )),
        vec![],
    )?;

    Ok(app
        .response("deposit-one")
        .add_submessages(msgs)
        .add_message(last_step))
}

pub fn execute_one_deposit_step(
    deps: DepsMut,
    _env: Env,
    asset_in: Coin,
    denom_out: String,
    expected_amount: Uint128,
    app: App,
) -> AppResult {
    let config = CONFIG.load(deps.storage)?;
    deps.api.debug("Start onde deposit step");

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
    yield_type: YieldType,
    yield_index: usize,
    app: App,
) -> AppResult {
    deps.api.debug("Start finalize deposit");
    let available_deposit_coins = TEMP_DEPOSIT_COINS.load(deps.storage)?;

    TEMP_CURRENT_YIELD.save(deps.storage, &yield_index)?;

    let msgs = yield_type.deposit(deps.as_ref(), available_deposit_coins, &app)?;

    Ok(app.response("finalize-deposit").add_submessages(msgs))
}

pub fn save_strategy(deps: DepsMut, mut strategy: Strategy) -> AppResult<()> {
    // We need to correct positions for which the cache is not empty
    // This is a security measure
    strategy
        .0
        .iter_mut()
        .for_each(|s| s.yield_source.ty.clear_cache());
    STRATEGY_CONFIG.save(deps.storage, &strategy)?;
    Ok(())
}
