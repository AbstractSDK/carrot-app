use abstract_app::abstract_sdk::AbstractResponse;
use cosmwasm_std::{DepsMut, Env, Reply, StdError, SubMsgResponse, SubMsgResult};
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgCreatePositionResponse;

use crate::{
    contract::{App, AppResult},
    error::AppError,
    state::CarrotPosition,
};

pub fn create_position_reply(deps: DepsMut, env: Env, app: App, reply: Reply) -> AppResult {
    let SubMsgResult::Ok(SubMsgResponse { data: Some(b), .. }) = reply.result else {
        return Err(AppError::Std(StdError::generic_err(
            "Failed to create position",
        )));
    };

    let parsed = cw_utils::parse_execute_response_data(&b)?;

    // Parse create position response
    let response: MsgCreatePositionResponse = parsed.data.clone().unwrap_or_default().try_into()?;

    // We save the position
    CarrotPosition::save_position(deps, env, response.position_id)?;

    Ok(app
        .response("create_position_reply")
        .add_attribute("initial_position_id", response.position_id.to_string()))
}
