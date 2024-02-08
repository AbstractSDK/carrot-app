use abstract_client::{AbstractClient, AccountSource, Environment};
use app::{
    msg::{AppExecuteMsg, ExecuteMsg},
    AppInterface,
};
use cosmos_sdk_proto::{cosmwasm::wasm::v1::{
    query_client::QueryClient, QueryContractsByCodeRequest,
}, traits::Message as _};
use cw_orch::{
    anyhow,
    daemon::{networks::OSMO_5, queriers::Authz, Daemon},
    environment::TxHandler,
    prelude::*,
    tokio::runtime::Runtime,
};
use log::{log, Level};
use osmosis_std::types::{cosmos::{authz::v1beta1::GenericAuthorization, bank::v1beta1::{MsgSend, SendAuthorization}}, osmosis::{concentratedliquidity::v1beta1::{MsgAddToPosition, MsgCollectIncentives, MsgCollectSpreadRewards, MsgCreatePosition, MsgWithdrawPosition}, gamm::v1beta1::MsgSwapExactAmountIn}};
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

const AUTHORIZATION_URLS: &[&str] = &[
    MsgCreatePosition::TYPE_URL,
    MsgSwapExactAmountIn::TYPE_URL,
    MsgAddToPosition::TYPE_URL,
    MsgWithdrawPosition::TYPE_URL,
    MsgCollectIncentives::TYPE_URL,
    MsgCollectSpreadRewards::TYPE_URL,
];

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

/// Finds the account owner and checks if the contract has authz permissions on it.
pub fn has_authz_permission(
    abstr: &AbstractClient<Daemon>,
    contract_addr: &String,
) -> anyhow::Result<bool> {
    let daemon = abstr.environment();

    let account = abstr.account_from(AccountSource::App(Addr::unchecked(contract_addr)))?;
    let granter = account.owner()?;

   // Check if authz is indeed given
   let authz_querier: Authz = daemon.querier();
   let authz_grantee = contract_addr.to_string();
   let generic_authorizations: Vec<GenericAuthorization> = daemon
       .rt_handle
       .block_on(async {
           authz_querier
               ._grants(
                   granter.to_string(),
                   authz_grantee.clone(),
                   GenericAuthorization::TYPE_URL.to_string(),
                   None,
               )
               .await
       })?
       .grants
       .into_iter()
       .map(|grant| GenericAuthorization::decode(&*grant.authorization.unwrap().value))
       .collect::<Result<_, _>>()?;
   // Check all generic authorizations are in place
   for &authorization_url in AUTHORIZATION_URLS {
       if !generic_authorizations.contains(&GenericAuthorization {
           msg: authorization_url.to_owned(),
       }) {
           return Ok(false)
       }
   }

   // Check any of send authorization is in place
   if !generic_authorizations.contains(&GenericAuthorization {
       msg: MsgSend::TYPE_URL.to_owned(),
   }) {
       let send_authorizations = daemon.rt_handle.block_on(async {
           authz_querier
               ._grants(
                   granter.to_string(),
                   authz_grantee,
                   SendAuthorization::TYPE_URL.to_string(),
                   None,
               )
               .await
       })?;
       if send_authorizations.grants.is_empty() {
           return Ok(false)
       }
   }
    Ok(true)
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
