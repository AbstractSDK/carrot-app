use abstract_app::abstract_sdk::AbstractResponse;
use cosmwasm_std::{Binary, DepsMut, Env, Reply};
use osmosis_std::types::{
    cosmos::authz::v1beta1::MsgExecResponse,
    osmosis::concentratedliquidity::v1beta1::MsgAddToPositionResponse,
};

use crate::{
    contract::{App, AppResult},
    helpers::get_user,
    state::{Position, POSITION},
};

pub fn add_to_position_reply(deps: DepsMut, env: Env, app: App, reply: Reply) -> AppResult {
    // Parse the msg exec response from the reply
    let authz_response: MsgExecResponse = reply.result.try_into()?;

    // Parse the position response from the first authz message
    let response: MsgAddToPositionResponse =
        Binary(authz_response.results[0].clone()).try_into()?;

    // We get the recipient of the position
    let recipient = get_user(deps.as_ref(), &app)?;

    // We update the position
    let position = Position {
        owner: recipient.to_string(),
        position_id: response.position_id,
        last_compound: env.block.time,
    };

    POSITION.save(deps.storage, &position)?;

    Ok(app
        .response("create_position_reply")
        .add_attribute("updated_position_id", response.position_id.to_string()))
}
