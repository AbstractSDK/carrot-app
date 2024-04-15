use abstract_app::abstract_sdk::AbstractResponse;
use cosmwasm_std::{DepsMut, Env, Reply, StdError, SubMsgResponse, SubMsgResult};
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgAddToPositionResponse;

use crate::{
    contract::{App, AppResult},
    error::AppError,
    handlers::internal::save_strategy,
    state::{STRATEGY_CONFIG, TEMP_CURRENT_YIELD},
    yield_sources::yield_type::YieldType,
};

pub fn add_to_position_reply(deps: DepsMut, _env: Env, app: App, reply: Reply) -> AppResult {
    let SubMsgResult::Ok(SubMsgResponse { data: Some(b), .. }) = reply.result else {
        return Err(AppError::Std(StdError::generic_err(
            "Failed to create position",
        )));
    };

    // Parse the msg exec response from the reply. This is because this reply is generated by calling the proxy contract
    let parsed = cw_utils::parse_execute_response_data(&b)?;

    // Parse the position response from the message
    let response: MsgAddToPositionResponse = parsed.data.unwrap_or_default().try_into()?;

    // We update the position
    let current_position_index = TEMP_CURRENT_YIELD.load(deps.storage)?;
    let mut strategy = STRATEGY_CONFIG.load(deps.storage)?;

    let current_yield = strategy.0.get_mut(current_position_index).unwrap();

    current_yield.yield_source.params = match current_yield.yield_source.params.clone() {
        YieldType::ConcentratedLiquidityPool(mut position) => {
            position.position_id = Some(response.position_id);
            YieldType::ConcentratedLiquidityPool(position)
        }
        YieldType::Mars(_) => return Err(AppError::WrongYieldType {}),
    };
    deps.api.debug("after add");

    save_strategy(deps, strategy)?;

    Ok(app
        .response("create_position_reply")
        .add_attribute("updated_position_id", response.position_id.to_string()))
}