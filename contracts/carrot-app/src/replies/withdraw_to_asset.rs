use abstract_app::abstract_sdk::AbstractResponse;
use cosmwasm_std::{DepsMut, Env, Reply, StdError, SubMsgResponse, SubMsgResult};
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgWithdrawPositionResponse;

use crate::{
    contract::{App, AppResult},
    error::AppError,
    handlers::swap_helpers::swap_msg,
    state::{CONFIG, TEMP_WITHDRAW_TO_ASSET},
};

pub fn withdraw_to_asset_reply(deps: DepsMut, env: Env, app: App, reply: Reply) -> AppResult {
    let SubMsgResult::Ok(SubMsgResponse { data: Some(b), .. }) = reply.result else {
        return Err(AppError::Std(StdError::generic_err(
            "Failed to withdraw to asset",
        )));
    };

    // Parse the msg exec response from the reply
    let parsed = cw_utils::parse_execute_response_data(&b)?;

    // Parse the position response from the message
    let response: MsgWithdrawPositionResponse = parsed.data.unwrap_or_default().try_into()?;

    let config = CONFIG.load(deps.storage)?;
    let payload = TEMP_WITHDRAW_TO_ASSET.load(deps.storage)?;

    let mut swap_msgs = vec![];
    if config.pool_config.asset0 != payload.expected_return.name {
        swap_msgs.extend(swap_msg(
            deps.as_ref(),
            &env,
            abstract_app::objects::AnsAsset {
                name: config.pool_config.asset0,
                amount: response.amount0.parse()?,
            },
            payload.expected_return.name.clone(),
            payload.max_spread,
            &app,
        )?);
    }
    if config.pool_config.asset1 != payload.expected_return.name {
        swap_msgs.extend(swap_msg(
            deps.as_ref(),
            &env,
            abstract_app::objects::AnsAsset {
                name: config.pool_config.asset1,
                amount: response.amount1.parse()?,
            },
            payload.expected_return.name,
            payload.max_spread,
            &app,
        )?);
    }
    Ok(app
        .response("withdraw_to_asset_reply")
        .add_messages(swap_msgs))
}
