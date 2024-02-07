use abstract_client::{AbstractClient, AccountSource, Environment};
use app::{
    msg::{AppExecuteMsg, ExecuteMsg},
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
use log::{log, Level};
use std::collections::HashSet;
use tonic::transport::Channel;

use abstract_app::{
    abstract_core::version_control::ModuleFilter,
    abstract_interface::VCQueryFns,
    objects::{
        module::{ModuleId, ModuleStatus},
        namespace::ABSTRACT_NAMESPACE,
    },
};
use app::contract::APP_ID;

use std::iter::Extend;

/// entrypoint for the bot
pub fn cron_main() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    let mut daemon = Daemon::builder()
        .handle(rt.handle())
        .chain(OSMO_5)
        .build()?;

    let abstr = AbstractClient::new(daemon.clone())?;
    let _module_id = ModuleId::try_from(APP_ID)?;

    let saving_modules = abstr.version_control().module_list(
        Some(ModuleFilter {
            namespace: Some(ABSTRACT_NAMESPACE.to_string()),
            name: Some("savings-app".to_string()),
            version: None,
            status: Some(ModuleStatus::Registered),
        }),
        None,
        None,
    )?;

    let mut contract_instances_to_check: HashSet<(String, Addr)> = HashSet::new();

    for app_info in saving_modules.modules {
        let code_id = app_info.module.reference.unwrap_app()?;
        let mut contract_addrs = rt
            .handle()
            .block_on(fetch_instances(daemon.channel(), code_id))?;

        // Only keep the contract addresses that have the required permissions
        contract_addrs.retain(|address| {
            has_authz_permission(&abstr, address)
                .ok()
                // Don't include if queries fail.
                .unwrap_or_default()
        });

        // Add all the entries to the `contract_instances_to_check`
        contract_instances_to_check.extend(
            contract_addrs
                .into_iter()
                .map(|addr| (app_info.module.info.to_string(), Addr::unchecked(addr))),
        );
    }

    // Run long-running autocompound job.
    autocompound(&mut daemon, &mut contract_instances_to_check)?;
    Ok(())
}

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

/// Finds the account owner and checks if the contract has authz permissions on it.
pub fn has_authz_permission(
    abstr: &AbstractClient<Daemon>,
    contract_addr: &String,
) -> anyhow::Result<bool> {
    let daemon = abstr.environment();

    let account = abstr.account_from(AccountSource::App(Addr::unchecked(contract_addr)))?;
    // Check if grant is indeed given
    let authz_granter = account.owner()?;
    let authz_querier: Authz = daemon.querier();
    let grantee = daemon.sender().to_string();
    let grants = daemon.rt_handle.block_on(async {
        authz_querier
            ._grants(
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
    for (id, address) in instances_to_check.iter() {
        let app = AppInterface::new(id, daemon.clone());
        app.set_address(address);
        use app::AppQueryMsgFns;
        let resp = app.available_rewards()?;

        // TODO: ensure rewards > tx fee

        if !resp.available_rewards.is_empty() {
            // Execute autocompound
            let daemon = daemon.rebuild().authz_granter(address).build()?;
            daemon.execute(
                &ExecuteMsg::from(AppExecuteMsg::Autocompound {}),
                &[],
                &address,
            )?;
        }
    }
    Ok(())
}
