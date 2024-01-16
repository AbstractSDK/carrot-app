use crate::contract::{App, AppResult};
use crate::error::AppError;
use crate::msg::{AppExecuteMsg, ExecuteMsg};
use crate::replies::SUCCESSFUL_DEPOSIT_REPLY_ID;
use crate::state::{CONFIG, STATE};
use abstract_sdk::features::AbstractResponse;
use cosmwasm_std::{
    to_json_binary, CosmosMsg, DepsMut, Env, MessageInfo, QueryRequest, Response, SubMsg, Uint128,
    WasmMsg,
};
use cw_asset::AssetList;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    MsgAddToPosition, MsgCollectIncentives, MsgCollectSpreadRewards, MsgCreatePosition,
    MsgWithdrawPosition, PositionByIdRequest, PositionByIdResponse,
};
use prost::Message;

pub fn execute_handler(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    app: App,
    msg: AppExecuteMsg,
) -> AppResult {
    match msg {
        AppExecuteMsg::Deposit {} => deposit(deps, env, info, app),
        AppExecuteMsg::Withdraw { amount } => withdraw(deps, env, info, Some(amount), app),
        AppExecuteMsg::WithdrawAll {} => withdraw(deps, env, info, None, app),
        AppExecuteMsg::Autocompound {} => autocompound(deps, env, info, app),
        AppExecuteMsg::Restake {} => restake(deps, env, info, app),
    }
}

fn deposit(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Only the authorized addresses (admin ?) can deposit
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;

    let config = CONFIG.load(deps.storage)?;
    let deposited_assets = AssetList::from(&info.funds);
    let deposited_amount = match deposited_assets.find(&config.deposit_info) {
        Some(asset) => asset.amount,
        None => {
            return Err(AppError::DepositError {
                expected: config.deposit_info,
                got: info.funds,
            })
        }
    };

    let deposit_msg = _inner_deposit(deps, &env, deposited_amount, &app)?;

    Ok(app
        .tag_response(Response::default(), "increment")
        .add_submessage(deposit_msg))
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

    let (withdraw_msg, withdraw_amount) = _inner_withdraw(deps, &env, amount, &app)?;

    Ok(app
        .tag_response(Response::default(), "withdraw")
        .add_attribute("withdraw_amount", withdraw_amount)
        .add_message(withdraw_msg))
}

fn autocompound(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Only the authorized addresses (admin ?) can auto-compound
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;

    // We start by withdrawing spread and incentives rewards
    let state = STATE.load(deps.storage)?;
    let position_id = state.current_position_id.ok_or(AppError::NoPosition {})?;
    let msg_spread = CosmosMsg::Stargate {
        value: MsgCollectSpreadRewards {
            position_ids: vec![position_id],
            sender: env.contract.address.to_string(),
        }
        .encode_to_vec()
        .into(),
        type_url: MsgCollectSpreadRewards::TYPE_URL.to_string(),
    };
    let msg_incentives = CosmosMsg::Stargate {
        value: MsgCollectIncentives {
            position_ids: vec![position_id],
            sender: env.contract.address.to_string(),
        }
        .encode_to_vec()
        .into(),
        type_url: MsgCollectIncentives::TYPE_URL.to_string(),
    };

    let msg_restake: CosmosMsg<_> = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::Restake {}))?,
        funds: vec![],
    });

    Ok(app
        .tag_response(Response::default(), "auto-compound")
        .add_message(msg_spread)
        .add_message(msg_incentives)
        .add_message(msg_restake))
}

fn restake(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Only the authorized addresses (admin ?) can auto-compound
    if env.contract.address != info.sender {
        return Err(AppError::Unauthorized {});
    }

    Ok(app.tag_response(Response::default(), "restake"))
}

fn _inner_deposit(deps: DepsMut, env: &Env, amount: Uint128, app: &App) -> AppResult<SubMsg> {
    let config = CONFIG.load(deps.storage)?;
    let state = STATE.load(deps.storage)?;

    // First we query the pool to know the ratio we can provide liquidity at :
    let pool_info = deps
        .querier
        .query(&QueryRequest::Stargate { path: (), data: () })?;

    let msg = if let Some(position_id) = state.current_position_id {
        // A position already exists, we simply extend it
        let msg = MsgAddToPosition {
            position_id,
            sender: env.contract.address.to_string(),
            amount0: todo!(),
            amount1: todo!(),
            token_min_amount0: todo!(),
            token_min_amount1: todo!(),
        };
        SubMsg::new(msg)
    } else {
        let msg = MsgCreatePosition {
            pool_id: config.pool_id,
            sender: env.contract.address.to_string(),
            lower_tick: config.lower_tick.i64(),
            upper_tick: config.upper_tick.i64(),
            tokens_provided: vec![osmosis_std::types::cosmos::base::v1beta1::Coin {
                denom: config.deposit_denom()?,
                amount: amount.to_string(),
            }],
            token_min_amount0: todo!(),
            token_min_amount1: todo!(),
        };
        SubMsg::reply_on_success(
            CosmosMsg::Stargate {
                type_url: MsgCreatePosition::TYPE_URL.to_string(),
                value: msg.encode_to_vec().into(),
            },
            SUCCESSFUL_DEPOSIT_REPLY_ID,
        )
    };

    Ok(msg)
}

fn _inner_withdraw(
    deps: DepsMut,
    env: &Env,
    amount: Option<Uint128>,
    app: &App,
) -> AppResult<(CosmosMsg, String)> {
    let state = STATE.load(deps.storage)?;
    let position_id = state.current_position_id.ok_or(AppError::NoPosition {})?;

    let liquidity_amount = if let Some(amount) = amount {
        amount.to_string()
    } else {
        let position_info: PositionByIdResponse = deps.querier.query(&QueryRequest::Stargate {
            path: PositionByIdRequest::TYPE_URL.to_string(),
            data: PositionByIdRequest { position_id }.encode_to_vec().into(),
        })?;

        position_info.position.unwrap().position.unwrap().liquidity
    };

    let msg = MsgWithdrawPosition {
        position_id,
        sender: env.contract.address.to_string(),
        liquidity_amount: liquidity_amount.clone(),
    };
    Ok((
        CosmosMsg::Stargate {
            type_url: MsgWithdrawPosition::TYPE_URL.to_string(),
            value: msg.encode_to_vec().into(),
        },
        liquidity_amount,
    ))
}
