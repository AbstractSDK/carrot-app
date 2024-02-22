use abstract_app::abstract_sdk::AbstractResponse;
use cosmwasm_std::{from_json, DepsMut, Env, Reply};
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgCreatePositionResponse;

use crate::{
    contract::{App, AppResult},
    helpers::get_user,
    state::{Position, POSITION},
};

pub fn create_position_reply(deps: DepsMut, env: Env, app: App, reply: Reply) -> AppResult {
    let b = reply
        .result
        .unwrap()
        .data
        .expect("Failed to create position");
    // Parse the msg exec response from the reply
    let parsed = cw_utils::parse_execute_response_data(&b)?;
    let response: MsgCreatePositionResponse = from_json(parsed.data.unwrap_or_default())?;

    // We get the creator of the position
    let creator = get_user(deps.as_ref(), &app)?;

    // We save the position
    let position = Position {
        owner: creator,
        position_id: response.position_id,
        last_compound: env.block.time,
    };

    POSITION.save(deps.storage, &position)?;

    Ok(app
        .response("create_position_reply")
        .add_attribute("initial_position_id", response.position_id.to_string()))
}
