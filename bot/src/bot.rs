use abstract_client::{AbstractClient, AccountSource, Environment};
use carrot_app::{
    msg::{AppExecuteMsg, CompoundStatusResponse, ExecuteMsg},
    AppInterface,
};
use cosmos_sdk_proto::{
    cosmwasm::wasm::v1::{query_client::QueryClient, QueryContractsByCodeRequest},
    traits::Message as _,
};
use cw_orch::{
    anyhow,
    daemon::{queriers::Authz, Daemon},
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

const AUTHORIZATION_URLS: &[&str] = &[
    MsgCreatePosition::TYPE_URL,
    MsgSwapExactAmountIn::TYPE_URL,
    MsgAddToPosition::TYPE_URL,
    MsgWithdrawPosition::TYPE_URL,
    MsgCollectIncentives::TYPE_URL,
    MsgCollectSpreadRewards::TYPE_URL,
];
use prometheus::{IntCounter, IntGauge, Registry};

pub struct Bot {
    pub daemon: Daemon,
    // Fetch information
    module_info: ModuleInfo,
    fetch_contracts_cooldown: Duration,
    last_fetch: SystemTime,
    // Autocompound information
    contract_instances_to_ac: HashSet<(String, Addr)>,
    pub autocompound_cooldown: Duration,
    // metrics
    metrics: Metrics,
}

struct Metrics {
    fetch_count: IntCounter,
    fetch_instances_count: IntGauge,
    autocompounded_count: IntCounter,
    autocompounded_error_count: IntCounter,
    contract_instances_to_autocompound: IntGauge,
}

impl Metrics {
    fn new(registry: &Registry) -> Self {
        let fetch_count = IntCounter::new(
            "carrot_app_bot_fetch_count",
            "Number of times the bot has fetched the instances",
        )
        .unwrap();
        let fetch_instances_count = IntGauge::new(
            "carrot_app_bot_fetch_instances_count",
            "Number of fetched instances",
        )
        .unwrap();
        let autocompounded_count = IntCounter::new(
            "carrot_app_bot_autocompounded_count",
            "Number of times contracts have been autocompounded",
        )
        .unwrap();
        let autocompounded_error_count = IntCounter::new(
            "carrot_app_bot_autocompounded_error_count",
            "Number of times autocompounding errored",
        )
        .unwrap();
        let contract_instances_to_autocompound = IntGauge::new(
            "carrot_app_bot_contract_instances_to_autocompound",
            "Number of instances that are eligible to be compounded",
        )
        .unwrap();
        registry.register(Box::new(fetch_count.clone())).unwrap();
        registry
            .register(Box::new(fetch_instances_count.clone()))
            .unwrap();
        registry
            .register(Box::new(autocompounded_count.clone()))
            .unwrap();
        registry
            .register(Box::new(autocompounded_error_count.clone()))
            .unwrap();
        registry
            .register(Box::new(contract_instances_to_autocompound.clone()))
            .unwrap();
        Self {
            fetch_count,
            fetch_instances_count,
            autocompounded_count,
            autocompounded_error_count,
            contract_instances_to_autocompound,
        }
    }
}

impl Bot {
    pub fn new(
        daemon: Daemon,
        module_info: ModuleInfo,
        fetch_contracts_cooldown: Duration,
        autocompound_cooldown: Duration,
        registry: &Registry,
    ) -> Self {
        let metrics = Metrics::new(registry);

        Self {
            daemon,
            module_info,
            fetch_contracts_cooldown,
            last_fetch: SystemTime::UNIX_EPOCH,
            contract_instances_to_ac: Default::default(),
            autocompound_cooldown,
            metrics,
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
        let abstr = AbstractClient::new(self.daemon.clone())?;
        let mut contract_instances_to_autocompound: HashSet<(String, Addr)> = HashSet::new();

        let saving_modules = abstr.version_control().module_list(
            Some(ModuleFilter {
                namespace: Some(self.module_info.namespace.to_string()),
                name: Some(self.module_info.name.clone()),
                version: None,
                status: Some(ModuleStatus::Registered),
            }),
            None,
            None,
        )?;
        let mut fetch_instances_count = 0;

        for app_info in saving_modules.modules {
            let code_id = app_info.module.reference.unwrap_app()?;

            let mut contract_addrs = daemon
                .rt_handle
                .block_on(utils::fetch_instances(daemon.channel(), code_id))?;
            fetch_instances_count += contract_addrs.len();

            // Only keep the contract addresses that have the required permissions
            contract_addrs.retain(|address| {
                utils::has_authz_permission(&abstr, address)
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

        // Metrics
        self.metrics.fetch_count.inc();
        self.metrics
            .fetch_instances_count
            .set(fetch_instances_count as i64);
        self.contract_instances_to_ac
            .clone_from(&contract_instances_to_autocompound);
        self.metrics
            .contract_instances_to_autocompound
            .set(contract_instances_to_autocompound.len() as i64);
        Ok(())
    }

    // Autocompound all saved instances and wait for cooldown duration
    pub fn autocompound(&self) {
        for (id, addr) in self.contract_instances_to_ac.iter() {
            let result = autocompound_instance(&self.daemon, (id, addr));
            if let Err(err) = result {
                log!(Level::Error, "error ocurred for {addr} carrot-app: {err:?}");
                self.metrics.autocompounded_error_count.inc();
            } else {
                self.metrics.autocompounded_count.inc();
            }
        }
    }
}

fn autocompound_instance(daemon: &Daemon, instance: (&str, &Addr)) -> anyhow::Result<()> {
    let (id, address) = instance;
    let app = AppInterface::new(id, daemon.clone());
    app.set_address(address);
    use carrot_app::AppQueryMsgFns;
    let resp: CompoundStatusResponse = app.compound_status()?;

    // TODO: ensure rewards > tx fee

    // Ensure there is rewards and pool rewards not empty
    if resp.autocompound_reward_available && !resp.pool_rewards.is_empty() {
        // Execute autocompound
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

        let grants = daemon
            .rt_handle
            .block_on(async {
                authz_querier
                    ._grants(
                        granter.to_string(),
                        authz_grantee.clone(),
                        // Get every authorization
                        "".to_owned(),
                        None,
                    )
                    .await
            })?
            .grants;
        let generic_authorizations: Vec<GenericAuthorization> = grants
            .iter()
            .filter_map(|grant| {
                GenericAuthorization::decode(&*grant.authorization.clone().unwrap().value).ok()
            })
            .collect();
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
            let send_authorizations: Vec<SendAuthorization> = grants
                .iter()
                .filter_map(|grant| {
                    SendAuthorization::decode(&*grant.authorization.clone().unwrap().value).ok()
                })
                .collect();
            if send_authorizations.is_empty() {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
