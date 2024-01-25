use cosmwasm_std::{CosmosMsg, Deps, Env, WasmMsg};
use osmosis_std::{
    cosmwasm_to_proto_coins,
    shim::Any,
    types::{cosmos::authz::v1beta1::MsgExec, cosmwasm::wasm::v1::MsgExecuteContract},
};

use prost::Message;

use crate::{
    contract::{App, AppResult},
    error::AppError,
};

pub fn wrap_authz(
    msg: impl Into<CosmosMsg<cosmwasm_std::Empty>>,
    sender: String,
    env: &Env,
) -> CosmosMsg {
    let msg = msg.into();
    let (type_url, value) = match msg {
        CosmosMsg::Wasm(wasm_msg) => match wasm_msg {
            WasmMsg::Execute {
                contract_addr,
                msg,
                funds,
            } => (
                MsgExecuteContract::TYPE_URL.to_string(),
                MsgExecuteContract {
                    sender,
                    contract: contract_addr,
                    msg: msg.into(),
                    funds: cosmwasm_to_proto_coins(funds),
                }
                .encode_to_vec(),
            ),
            not_supported => unimplemented!("{not_supported:?}"),
        },
        CosmosMsg::Stargate { type_url, value } => (type_url.clone(), value.into()),
        // CosmosMsg::Bank(bank_msg) => match bank_msg {
        //     cosmwasm_std::BankMsg::Send { to_address, amount } => (
        //         MsgSend::TYPE_URL.to_string(),
        //         MsgSend {
        //             from_address: sender.to_string(),
        //             to_address,
        //             amount: cosmwasm_to_proto_coins(amount),
        //         }
        //         .encode_to_vec(),
        //     ),
        //     not_supported => unimplemented!("{not_supported:?}"),
        // },
        not_supported => unimplemented!("{not_supported:?}"),
    };

    CosmosMsg::Stargate {
        type_url: MsgExec::TYPE_URL.to_string(),
        value: MsgExec {
            grantee: env.contract.address.to_string(),
            msgs: vec![Any { type_url, value }],
        }
        .encode_to_vec()
        .into(),
    }
}

pub fn get_user(deps: Deps, app: &App) -> AppResult<String> {
    app.admin
        .query_account_owner(deps)?
        .admin
        .ok_or(AppError::NoTopLevelAccount {})
}
