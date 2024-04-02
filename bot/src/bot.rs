use abstract_client::{AbstractClient, AccountSource, Environment};
use carrot_app::{
    msg::{AppExecuteMsg, AppQueryMsg, CompoundStatusResponse, ExecuteMsg, QueryMsg},
    AppInterface,
};
use semver::VersionReq;

use crate::Metrics;
use cosmos_sdk_proto::{
    cosmwasm::wasm::v1::{query_client::QueryClient, QueryContractsByCodeRequest},
    traits::Message as _,
};

use cosmwasm_std::Uint128;
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
use prometheus::{labels, Registry};
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

const VERSION_REQ: &str = ">=0.2";

const AUTHORIZATION_URLS: &[&str] = &[
    MsgCreatePosition::TYPE_URL,
    MsgSwapExactAmountIn::TYPE_URL,
    MsgAddToPosition::TYPE_URL,
    MsgWithdrawPosition::TYPE_URL,
    MsgCollectIncentives::TYPE_URL,
    MsgCollectSpreadRewards::TYPE_URL,
];

const APR_REFERENCE_INSTANCE: &str =
    "osmo1rhvdhjxx25x3v4gan68h3n0an3wsa94zjj4yjnxc5yx2vt6q3scsfjgykp";
// TODO: Get these values from ans
const USDT_OSMOSIS_DENOM: &str =
    "ibc/4ABBEF4C8926DDDB320AE5188CFD63267ABBCEFC0583E4AE05D6E5AA2401DDAB";
const USDC_OSMOSIS_DENOM: &str =
    "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";

pub struct Bot {
    pub daemon: Daemon,
    // Fetch information
    module_info: ModuleInfo,
    fetch_contracts_cooldown: Duration,
    last_fetch: SystemTime,
    // Autocompound information
    contract_instances_to_ac: HashSet<(String, Addr)>,
    // Used for APR calculation
    apr_reference_contract: Addr,
    pub autocompound_cooldown: Duration,
    // metrics
    metrics: Metrics,
}

struct Balance {
    coins: Vec<ValuedCoin>,
}
impl Balance {
    fn new(coins: Vec<ValuedCoin>) -> Self {
        Self { coins }
    }
    fn calculate_usd_value(self) -> Uint128 {
        self.coins.iter().fold(Uint128::zero(), |acc, c| {
            acc + c.coin.amount.checked_mul(c.usd_value).unwrap()
        })
    }
}
struct ValuedCoin {
    coin: Coin,
    usd_value: Uint128,
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
            apr_reference_contract: Addr::unchecked(APR_REFERENCE_INSTANCE),
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

        let ref_contract = self.apr_reference_contract.clone();

        let ver_req = VersionReq::parse(VERSION_REQ).unwrap();
        for app_info in saving_modules.modules {
            let version = app_info.module.info.version.to_string();
            // Skip if version mismatches
            if semver::Version::parse(&version)
                .map(|v| !ver_req.matches(&v))
                .unwrap_or(false)
            {
                continue;
            }
            let code_id = app_info.module.reference.unwrap_app()?;

            let mut contract_addrs = daemon.rt_handle.block_on(utils::fetch_instances(
                daemon.channel(),
                code_id,
                &version,
            ))?;
            fetch_instances_count += contract_addrs.len();

            self.metrics.reference_contract_balance.set(
                utils::get_carrot_balance(daemon.clone(), &Addr::unchecked(ref_contract.clone()))
                    .unwrap_or(Uint128::zero())
                    .u128()
                    .try_into() // There is a risk of overflow here
                    .unwrap(),
            );

            let mut total_value_locked = Uint128::zero();
            for contract_addr in contract_addrs.iter() {
                let address = Addr::unchecked(contract_addr.clone());
                let balance_result = utils::get_carrot_balance(daemon.clone(), &address);
                let balance = match balance_result {
                    Ok(value) => value,
                    Err(_) => Uint128::zero(), // Handle potential errors
                };

                // Update total_value_locked
                total_value_locked += balance;

                // Update contract_balance GaugeVec with label
                let label = labels! {"contract_address"=> contract_addr.as_ref()};
                self.metrics
                    .contract_balance
                    .with(&label)
                    .set(balance.u128().try_into().unwrap());
            }

            // Finally, set the total_value_locked metric after the loop
            self.metrics
                .total_value_locked
                .set(total_value_locked.u128().try_into().unwrap());

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
    pub async fn fetch_instances(
        channel: Channel,
        code_id: u64,
        version: &str,
    ) -> anyhow::Result<Vec<String>> {
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
        log!(Level::Info, "Savings addrs({version}): {contract_addrs:?}");
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

    /// gets the balance managed by an instance
    pub fn get_carrot_balance(daemon: Daemon, contract_addr: &Addr) -> anyhow::Result<Uint128> {
        let response: carrot_app::msg::AssetsBalanceResponse =
            daemon.query(&QueryMsg::from(AppQueryMsg::Balance {}), contract_addr)?;

        let balance = Balance::new(
            response
                .balances
                .iter()
                .filter(|c| c.denom.eq(USDT_OSMOSIS_DENOM) || c.denom.eq(USDC_OSMOSIS_DENOM))
                .map(|c| match c.denom.as_str() {
                    USDT_OSMOSIS_DENOM => ValuedCoin {
                        coin: c.clone(),
                        usd_value: Uint128::one(),
                    },
                    USDC_OSMOSIS_DENOM => ValuedCoin {
                        coin: c.clone(),
                        usd_value: Uint128::one(),
                    },
                    _ => ValuedCoin {
                        coin: c.clone(),
                        usd_value: Uint128::zero(),
                    },
                })
                .collect(),
        );
        let balance = balance.calculate_usd_value();

        log!(
            Level::Info,
            "contract: {contract_addr:?} balance: {balance:?}"
        );
        Ok(balance)
    }
}
