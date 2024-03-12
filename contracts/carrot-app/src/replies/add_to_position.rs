use abstract_app::abstract_sdk::AbstractResponse;
use cosmwasm_std::{DepsMut, Env, Reply, StdError, SubMsgResponse, SubMsgResult};
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgAddToPositionResponse;

use crate::{
    contract::{App, AppResult},
    error::AppError,
    helpers::get_user,
    state::{Position, POSITION},
};

pub fn add_to_position_reply(deps: DepsMut, env: Env, app: App, reply: Reply) -> AppResult {
    let SubMsgResult::Ok(SubMsgResponse { data: Some(b), .. }) = reply.result else {
        return Err(AppError::Std(StdError::generic_err(
            "Failed to create position",
        )));
    };

    // Parse the msg exec response from the reply
    let parsed = cw_utils::parse_execute_response_data(&b)?;

    // Parse the position response from the message
    let response: MsgAddToPositionResponse = parsed.data.unwrap_or_default().try_into()?;

    // We get the creator of the position
    let creator = get_user(deps.as_ref(), &app)?;

    // We update the position
    let position = Position {
        owner: creator,
        position_id: response.position_id,
        last_compound: env.block.time,
    };

    POSITION.save(deps.storage, &position)?;

    Ok(app
        .response("create_position_reply")
        .add_attribute("updated_position_id", response.position_id.to_string()))
}
