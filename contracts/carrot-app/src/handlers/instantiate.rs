use crate::{
    check::Checkable,
    contract::{App, AppResult},
    msg::AppInstantiateMsg,
    state::{save_strategy, CONFIG},
};
use abstract_app::abstract_sdk::AbstractResponse;
use cosmwasm_std::{DepsMut, Env, MessageInfo};

use super::execute::_inner_deposit;

pub fn instantiate_handler(
    mut deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    app: App,
    msg: AppInstantiateMsg,
) -> AppResult {
    // Check validity of registered config
    let config = msg.config.check(deps.as_ref(), &app)?;

    CONFIG.save(deps.storage, &config)?;
    let mut strategy = msg.strategy.check(deps.as_ref(), &app)?;
    save_strategy(deps.branch(), &mut strategy)?;

    let mut response = app.response("instantiate_savings_app");

    // If provided - do an initial deposit
    if let Some(funds) = msg.deposit {
        let deposit_msgs = _inner_deposit(deps.as_ref(), &env, funds, strategy, None, &app)?;

        response = response.add_messages(deposit_msgs);
    }
    Ok(response)
}
