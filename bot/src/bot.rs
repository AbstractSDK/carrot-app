use abstract_client::{AbstractClient, AccountSource, Environment};
use cosmos_sdk_proto::{
    cosmwasm::wasm::v1::{query_client::QueryClient, QueryContractsByCodeRequest},
    traits::Message as _,
};
use cw_orch::{
    anyhow,
    daemon::{queriers::Authz, Daemon},
    environment::TxHandler,
    prelude::*,
};
use log::{log, Level};
use osmosis_std::types::{
    cosmos::{
        authz::v1beta1::GenericAuthorization,
        bank::v1beta1::{MsgSend, SendAuthorization},
    },
    osmosis::{
        concentratedliquidity::v1beta1::{
            MsgAddToPosition, MsgCollectIncentives, MsgCollectSpreadRewards, MsgCreatePosition,
            MsgWithdrawPosition,
        },
        gamm::v1beta1::MsgSwapExactAmountIn,
    },
};
use savings_app::{
    msg::{AppExecuteMsg, CompoundStatusResponse, ExecuteMsg},
    AppInterface,
};
use std::{
    collections::HashSet,
    time::{Duration, SystemTime},
};
use tonic::transport::Channel;

use abstract_app::{
    abstract_core::version_control::ModuleFilter,
    abstract_interface::VCQueryFns,
    objects::module::{ModuleInfo, ModuleStatus},
};

use std::iter::Extend;

const AUTHORIZATION_URLS: &[&str] = &[
    MsgCreatePosition::TYPE_URL,
    MsgSwapExactAmountIn::TYPE_URL,
    MsgAddToPosition::TYPE_URL,
    MsgWithdrawPosition::TYPE_URL,
    MsgCollectIncentives::TYPE_URL,
    MsgCollectSpreadRewards::TYPE_URL,
];

pub struct Bot {
    abstract_client: AbstractClient<Daemon>,
    daemon: Daemon,
    // Fetch information
    module_info: ModuleInfo,
    fetch_contracts_cooldown: Duration,
    last_fetch: SystemTime,
    // Autocompound information
    contract_instances_to_ac: HashSet<(String, Addr)>,
    autocompound_cooldown: Duration,
}

impl Bot {
    pub fn new(
        abstract_client: AbstractClient<Daemon>,
        module_info: ModuleInfo,
        fetch_contracts_cooldown: Duration,
        autocompound_cooldown: Duration,
    ) -> Self {
        let daemon = abstract_client.environment();
        Self {
            abstract_client,
            daemon,
            module_info,
            fetch_contracts_cooldown,
            last_fetch: SystemTime::UNIX_EPOCH,
            contract_instances_to_ac: Default::default(),
            autocompound_cooldown,
        }
    }

    // Fetches contracts if fetch cooldown passed
    pub fn fetch_contracts(&mut self) -> anyhow::Result<()> {
        // Don't fetch if not ready
        let ready_time = self.last_fetch + self.fetch_contracts_cooldown;
        if SystemTime::now() < ready_time {
            return Ok(());
        }

        let daemon = &self.daemon;
        let mut contract_instances_to_autocompound: HashSet<(String, Addr)> = HashSet::new();

        let saving_modules = self.abstract_client.version_control().module_list(
            Some(ModuleFilter {
                namespace: Some(self.module_info.namespace.to_string()),
                name: Some(self.module_info.name.clone()),
                version: None,
                status: Some(ModuleStatus::Registered),
            }),
            None,
            None,
        )?;

        for app_info in saving_modules.modules {
            let code_id = app_info.module.reference.unwrap_app()?;
            let mut contract_addrs = daemon
                .rt_handle
                .block_on(utils::fetch_instances(daemon.channel(), code_id))?;

            // Only keep the contract addresses that have the required permissions
            contract_addrs.retain(|address| {
                utils::has_authz_permission(&self.abstract_client, address)
                    // Don't include if queries fail.
                    .unwrap_or_default()
            });

            // Add all the entries to the `contract_instances_to_check`
            contract_instances_to_autocompound.extend(
                contract_addrs
                    .into_iter()
                    .map(|addr| (app_info.module.info.id(), Addr::unchecked(addr))),
            );
        }

        self.contract_instances_to_ac = contract_instances_to_autocompound;
        Ok(())
    }

    // Autocompound all saved instances and wait for cooldown duration
    pub fn autocompound(&self) {
        for (id, addr) in self.contract_instances_to_ac.iter() {
            let result = autocompound_instance(&self.daemon, (id, addr));
            if let Err(err) = result {
                log!(
                    Level::Error,
                    "error ocurred for {addr} savings-app: {err:?}"
                );
            }
        }
        // Wait for autocompound duration
        std::thread::sleep(self.autocompound_cooldown);
    }
}

fn autocompound_instance(daemon: &Daemon, instance: (&str, &Addr)) -> anyhow::Result<()> {
    let (id, address) = instance;
    let app = AppInterface::new(id, daemon.clone());
    app.set_address(address);
    use savings_app::AppQueryMsgFns;
    let resp: CompoundStatusResponse = app.compound_status()?;

    // TODO: ensure rewards > tx fee
    // To discuss if we really need it?

    if resp.rewards_available {
        // Execute autocompound
        let daemon = daemon.rebuild().authz_granter(address).build()?;
        daemon.execute(
            &ExecuteMsg::from(AppExecuteMsg::Autocompound {}),
            &[],
            address,
        )?;
    }
    Ok(())
}

mod utils {
    use super::*;

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
                return Ok(false);
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
                return Ok(false);
            }
        }
        Ok(true)
    }
}
