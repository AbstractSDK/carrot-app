use abstract_app::abstract_sdk::AbstractResponse;
use cosmwasm_std::{Binary, DepsMut, Env, QueryRequest, Reply, StdError};
use osmosis_std::types::{
    cosmos::authz::v1beta1::MsgExecResponse,
    osmosis::concentratedliquidity::v1beta1::{
        ConcentratedliquidityQuerier, MsgCreatePositionResponse, UserPositionsRequest,
        UserPositionsResponse,
    },
};

use crate::{
    contract::{App, AppResult},
    helpers::get_user,
    state::{get_position, Position, CONFIG, POSITION},
};

pub fn create_position_reply(deps: DepsMut, env: Env, app: App, reply: Reply) -> AppResult {
    // We get the creator of the position
    let creator = get_user(deps.as_ref(), &app)?;
    let config = CONFIG.load(deps.storage)?;

    let position_resp = ConcentratedliquidityQuerier::new(&deps.querier).user_positions(
        creator.to_string(),
        config.pool_config.pool_id,
        None,
    )?;

    // let position_resp_bin: Binary = deps
    //     .querier
    //     .query(&QueryRequest::Stargate {
    //         path: UserPositionsRequest::TYPE_URL.to_string(),
    //         data: Binary(
    //             UserPositionsRequest {
    //                 address: creator.to_string(),
    //                 pool_id: config.pool_config.pool_id,
    //                 pagination: None,
    //             }
    //             .to_proto_bytes(),
    //         ),
    //     })
    //     .map_err(|e| StdError::generic_err(format!("stargate query err: {e}")))?;

    // let position_resp: UserPositionsResponse = position_resp_bin.try_into()?;

    let position_id = position_resp.positions[0]
        .clone()
        .position
        .unwrap()
        .position_id
        .clone();
    // We save the position
    let position = Position {
        owner: creator,
        position_id: position_id.clone(),
        last_compound: env.block.time,
    };

    POSITION.save(deps.storage, &position)?;

    Ok(app
        .response("create_position_reply")
        .add_attribute("initial_position_id", position_id.to_string()))
}
