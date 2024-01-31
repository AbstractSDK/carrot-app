use abstract_app::abstract_sdk::AbstractResponse;
use cosmwasm_std::{BankMsg, Coin, DepsMut, Env, Reply};

use crate::{
    contract::{App, AppResult},
    helpers::{get_user, wrap_authz},
    state::{CONFIG, CURRENT_EXECUTOR},
};

pub fn proxy_rewards(deps: DepsMut, env: Env, app: App, _reply: Reply) -> AppResult {
    let executor = CURRENT_EXECUTOR.load(deps.storage)?;
    let config = CONFIG.load(deps.storage)?;
    let rewards_config = config.autocompound_rewards_config;
    
    let user = get_user(deps.as_ref(), &app)?;
    // let bal = deps.querier.query_balance(user, rewards_config.gas_denom)?;
    // panic!("aloha: {bal}");
    
    let reward = Coin {
        denom: rewards_config.gas_denom,
        amount: rewards_config.reward,
    };
    // To avoid giving general `MsgSend` authorization we do 2 sends here
    // 1) From user to the contract
    // 2) From contract to the executor
    let msg_send = BankMsg::Send {
        to_address: env.contract.address.to_string(),
        amount: vec![reward.clone()],
    };
    let reward_into_contract = wrap_authz(msg_send, user, &env);

    let reward_into_executor = BankMsg::Send {
        to_address: executor.into_string(),
        amount: vec![reward],
    };

    Ok(app
        .response("proxy_rewards")
        .add_message(reward_into_contract)
        .add_message(reward_into_executor))
}
