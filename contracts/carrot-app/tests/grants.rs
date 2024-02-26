// // TODO: Do we REALLY need to use authz for this one tho?
// // TODO: Can we add Contract Grant to abstract
//
// use abstract_app::abstract_sdk::features::AccountIdentification;
// use abstract_app::abstract_sdk::AbstractResponse;
// use abstract_app::abstract_sdk::AccountAction;
// use abstract_app::abstract_sdk::Execution;
// use cosmwasm_std::to_json_binary;
// use cosmwasm_std::CosmosMsg;
// use cosmwasm_std::{DepsMut, Env, MessageInfo};
// use prost::Message;

// use crate::contract::{App, AppResult};
// use crate::msg;
// use crate::msg::AppInstantiateMsg;
// use crate::state::{Config, CONFIG};

// let contract_grant = osmosis_std::types::cosmwasm::wasm::v1::ContractGrant {
//     contract: env.contract.address.to_string(),
//     // Limitless
//     limit: None,
//     // Allow only autocompound
//     filter: Some(
//         osmosis_std::types::cosmwasm::wasm::v1::AcceptedMessagesFilter {
//             messages: vec![to_json_binary(&msg::AppExecuteMsg::Autocompound {})?.0],
//         }
//         .to_any(),
//     ),
// };
// let msg = osmosis_std::types::cosmos::authz::v1beta1::MsgGrant {
//     granter: app.proxy_address(deps.as_ref())?.to_string(),
//     grantee: config.bot_addr.to_string(),
//     grant: Some(osmosis_std::types::cosmos::authz::v1beta1::Grant {
//         authorization: Some(contract_grant.to_any()),
//         expiration: None,
//     }),
// }
// .encode_to_vec();

// let grant_msg = CosmosMsg::Stargate {
//     type_url: osmosis_std::types::cosmos::authz::v1beta1::MsgGrant::TYPE_URL.to_string(),
//     value: cosmwasm_std::Binary(msg),
// };
