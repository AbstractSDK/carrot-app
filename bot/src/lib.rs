use std::collections::HashSet;

use abstract_app::{
    abstract_core::{
        app::{AppConfigResponse, BaseQueryMsg},
        MANAGER,
    },
    abstract_interface::{Abstract, AbstractAccount, Manager, ManagerQueryFns},
};
use abstract_client::Account;
use app::{
    contract::App,
    msg::{AppExecuteMsg, AppQueryMsg, AvailableRewardsResponse, ExecuteMsg, QueryMsg},
    AppInterface,
};
use cosmos_sdk_proto::cosmwasm::wasm::v1::{
    query_client::QueryClient, QueryContractsByCodeRequest,
};
use cw_orch::{
    anyhow,
    daemon::{networks::OSMO_5, queriers::Authz, Daemon},
    environment::TxHandler,
    prelude::*,
    tokio::runtime::Runtime,
};
use dotenv::dotenv;
use log::{log, Level};
use tonic::transport::Channel;

/// Get the contract instances of a given code_id
pub async fn fetch_instances(channel: Channel, code_id: u64) -> anyhow::Result<Vec<String>> {
    let mut cw_querier = QueryClient::new(channel);
    let contract_addrs = cw_querier
        .contracts_by_code(QueryContractsByCodeRequest {
            code_id,
            // TODO: pagination
            pagination: None,
        })
        .await?
        .into_inner()
        .contracts;
    log!(Level::Info, "Savings addrs: {contract_addrs:?}");
    anyhow::Ok(contract_addrs)
}

// TODO: are we using wasm execute grant here?
const MSG_TYPE_URL: &str = "";

pub fn daemon_with_savings_authz(daemon: &Daemon, contract_addr: &Addr) -> anyhow::Result<Daemon> {
    // Get config of an app to get proxy address(granter)
    let app_config: AppConfigResponse =
        daemon.query(&QueryMsg::Base(BaseQueryMsg::BaseConfig {}), contract_addr)?;

    // Check if grant is indeed given
    let authz_granter = app_config.proxy_address;
    let authz_querier: Authz = daemon.query_client();
    let grantee = daemon.sender().to_string();
    let grants = daemon.rt_handle.block_on(async {
        authz_querier
            .grants(
                authz_granter.to_string(),
                grantee,
                MSG_TYPE_URL.to_string(),
                None,
            )
            .await
    })?;
    if grants.grants.is_empty() {
        return Err(anyhow::anyhow!("Missing required grant"));
    }

    Ok(daemon.with_authz_granter(authz_granter))
}

pub fn has_authz_permission(daemon: &Daemon, contract_addr: &Addr) -> anyhow::Result<bool> {
    // Get config of an app to get proxy address(granter)
    let app_config: AppConfigResponse =
        daemon.query(&QueryMsg::Base(BaseQueryMsg::BaseConfig {}), contract_addr)?;

    let manager = Manager::new(MANAGER, daemon.clone());
    manager.set_address(&app_config.manager_address);
    let acc_config = manager.config()?;

    let account = AbstractAccount::new(&Abstract::new(daemon.clone()), acc_config.account_id);
    Account::new(account,false);

    // Check if grant is indeed given
    let authz_granter = app_config.proxy_address;
    let authz_querier: Authz = daemon.query_client();
    let grantee = daemon.sender().to_string();
    let grants = daemon.rt_handle.block_on(async {
        authz_querier
            .grants(
                authz_granter.to_string(),
                grantee,
                MSG_TYPE_URL.to_string(),
                None,
            )
            .await
    })?;
    Ok(!grants.grants.is_empty())
}

pub fn autocompound(
    daemon: &mut Daemon,
    instances_to_check: &mut HashSet<(String, Addr)>,
) -> anyhow::Result<()> {
    let mut stale_apps = vec![];

    for (id, address) in instances_to_check.iter() {
        let app = AppInterface::new(id, daemon.clone());
        app.set_address(address);
        use app::AppQueryMsgFns;
        let resp = app.available_rewards()?;

        if !resp.available_rewards.is_empty() {
            // Execute autocompound, if we have grant(s)
            if let Ok(daemon) = daemon_with_savings_authz(daemon, &address) {
                daemon.execute(
                    &ExecuteMsg::from(AppExecuteMsg::Autocompound {}),
                    &[],
                    &address,
                )?;
            } else {
                // Remove app from the list, as we don't have grant
                stale_apps.push((id.clone(), address.clone()));
            }
        }
    }

    // We can try to batch it, but it could be PIAS to not gas overflow
    // daemon.daemon.sender.commit_tx(msgs, memo)
    Ok(())
}
