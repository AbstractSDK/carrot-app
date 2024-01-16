use abstract_sdk::AbstractResponse;
use cosmwasm_std::{DepsMut, Env, MessageInfo, Response};

use crate::contract::{App, AppResult};
use crate::msg::AppInstantiateMsg;
use crate::state::{Config, CONFIG};

pub fn instantiate_handler(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    app: App,
    msg: AppInstantiateMsg,
) -> AppResult {
    let config: Config = Config {
        deposit_info: cw_asset::AssetInfoBase::Native(msg.deposit_denom),
        quasar_pool: deps.api.addr_validate(&msg.quasar_pool)?,
        exchanges: msg.exchanges,
    };

    CONFIG.save(deps.storage, &config)?;

    // Example instantiation that doesn't do anything
    Ok(app.tag_response(Response::new(), "instantiate_savings_app"))
}
