use abstract_sdk::AbstractResponse;
use cl_vault::msg::ClQueryMsg;
use cl_vault::query::PoolResponse;
use cosmwasm_std::to_json_binary;
use cosmwasm_std::QueryRequest;
use cosmwasm_std::StdError;
use cosmwasm_std::WasmQuery;
use cosmwasm_std::{DepsMut, Env, MessageInfo};

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
    let quasar_pool_addr = deps.api.addr_validate(&msg.quasar_pool)?;
    // We query the pool information in advance to store that inside config
    let quasar_pool_response: PoolResponse = deps
        .querier
        .query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: quasar_pool_addr.to_string(),
            msg: to_json_binary(&crate::cl_vault::msg::QueryMsg::VaultExtension(
                cl_vault::msg::ExtensionQueryMsg::ConcentratedLiquidity(ClQueryMsg::Pool {}),
            ))?,
        }))
        .map_err(|_| StdError::generic_err("Failed to get pool info in instantiation"))?;

    let config: Config = Config {
        deposit_info: cw_asset::AssetInfoBase::Native(msg.deposit_denom),
        quasar_pool: quasar_pool_addr,
        exchanges: msg.exchanges,
        pool: quasar_pool_response.pool_config,
    };
    CONFIG.save(deps.storage, &config)?;

    Ok(app.response("instantiate_savings_app"))
}
