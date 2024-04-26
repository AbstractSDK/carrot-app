use abstract_client::{AbstractClient, AccountSource, Environment};
use carrot_app::{
    msg::{
        AppExecuteMsg, AppQueryMsg, CompoundStatus, CompoundStatusResponse, ExecuteMsg, QueryMsg,
    },
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
    fmt::{Display, Formatter},
    time::{Duration, SystemTime},
};
use tonic::transport::Channel;

use abstract_app::{
    abstract_core::version_control::ModuleFilter,
    abstract_interface::VCQueryFns,
    objects::module::{ModuleInfo, ModuleStatus},
};

const VERSION_REQ: &str = ">=0.3";

const AUTHORIZATION_URLS: &[&str] = &[
    MsgCreatePosition::TYPE_URL,
    MsgSwapExactAmountIn::TYPE_URL,
    MsgAddToPosition::TYPE_URL,
    MsgWithdrawPosition::TYPE_URL,
    MsgCollectIncentives::TYPE_URL,
    MsgCollectSpreadRewards::TYPE_URL,
];
// TODO: Get these values from ans
const USDT_OSMOSIS_DENOM: &str =
    "ibc/4ABBEF4C8926DDDB320AE5188CFD63267ABBCEFC0583E4AE05D6E5AA2401DDAB";
const USDC_OSMOSIS_DENOM: &str =
    "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
const USDC_OSMOSIS_AXL_DENOM: &str =
    "ibc/D189335C6E4A68B513C10AB227BF1C1D38C746766278BA3EEB4FB14124F1D858";

pub struct Bot {
    pub daemon: Daemon,
    // Fetch information
    module_info: ModuleInfo,
    fetch_contracts_cooldown: Duration,
    last_fetch: SystemTime,
    // Autocompound information
    contract_instances_to_ac: HashSet<(String, CarrotInstance)>,
    pub autocompound_cooldown: Duration,
    // metrics
    metrics: Metrics,
}

#[derive(Eq, Hash, PartialEq, Clone)]
struct CarrotInstance {
    address: Addr,
    version: String,
}
impl CarrotInstance {
    fn new(address: Addr, version: &str) -> Self {
        Self {
            address,
            version: version.to_string(),
        }
    }
}

impl Display for CarrotInstance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CarrotInstance {{ address: {:?}, version: {} }}",
            self.address, self.version
        )
    }
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
        let mut contract_instances_to_autocompound: HashSet<(String, CarrotInstance)> =
            HashSet::new();

        log!(Level::Debug, "Fetching modules");
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

        let ver_req = VersionReq::parse(VERSION_REQ).unwrap();
        log!(Level::Debug, "version requirement: {ver_req}");
        for app_info in saving_modules.modules {
            let version = app_info.module.info.version.to_string();

            let code_id = app_info.module.reference.unwrap_app()?;

            let mut contract_addrs = daemon.rt_handle.block_on(utils::fetch_instances(
                daemon.channel(),
                code_id,
                &version,
            ))?;
            fetch_instances_count += contract_addrs.len();

            for contract_addr in contract_addrs.iter() {
                let address = Addr::unchecked(contract_addr.clone());
                let balance_result = utils::get_carrot_balance(daemon.clone(), &address);
                let balance = match balance_result {
                    Ok(value) => value,
                    Err(_) => Uint128::zero(), // Handle potential errors
                };

                // Update contract_balance GaugeVec with label
                let label = labels! {"contract_address"=> contract_addr.as_ref(),"contract_version"=> version.as_ref()};

                self.metrics
                    .contract_balance
                    .with(&label)
                    .set(balance.u128().try_into().unwrap());
            }

            // Skip if version mismatches
            if semver::Version::parse(&version)
                .map(|v| !ver_req.matches(&v))
                .unwrap_or(false)
            {
                continue;
            }

            // Only keep the contract addresses that have the required permissions
            contract_addrs.retain(|address| {
                utils::has_authz_permission(&abstr, address)
                    // Don't include if queries fail.
                    .unwrap_or_default()
            });

            // Add all the entries to the `contract_instances_to_check`
            contract_instances_to_autocompound.extend(contract_addrs.into_iter().map(|addr| {
                (
                    app_info.module.info.id(),
                    CarrotInstance::new(Addr::unchecked(addr), version.as_ref()),
                )
            }));
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
        for (id, contract) in self.contract_instances_to_ac.iter() {
            let version = &contract.version;
            let addr = &contract.address;
            let label = labels! {"contract_version"=> version.as_ref()};
            match autocompound_instance(&self.daemon, (id, addr)) {
                // Successful autocompound
                Ok(CompoundStatus::Ready {}) => {
                    self.metrics.autocompounded_count.with(&label).inc()
                }
                // Checked contract not ready for autocompound
                Ok(status) => {
                    log!(Level::Info, "Skipped {contract}: {status:?}");
                    self.metrics
                        .autocompounded_not_ready_count
                        .with(&label)
                        .inc();
                }
                // Contract (or bot) errored during autocompound
                Err(err) => {
                    log!(
                        Level::Error,
                        "error ocurred for {contract} carrot-app: {err:?}"
                    );
                    self.metrics.autocompounded_error_count.with(&label).inc();
                }
            }
        }
    }
}

fn autocompound_instance(
    daemon: &Daemon,
    instance: (&str, &Addr),
) -> anyhow::Result<CompoundStatus> {
    let (id, address) = instance;
    let app = AppInterface::new(id, daemon.clone());
    app.set_address(address);
    use carrot_app::AppQueryMsgFns;
    let resp: CompoundStatusResponse = app.compound_status()?;

    // Ensure contract is ready for autocompound,
    // user can send rewards,
    // spread rewards not empty
    // and bot will get paid enough to cover gas fees
    if resp.status.is_ready()
        && resp.autocompound_reward_available
        && !resp.spread_rewards.is_empty()
        && utils::enough_rewards(resp.autocompound_reward)
    {
        // Execute autocompound
        daemon.execute(
            &ExecuteMsg::from(AppExecuteMsg::Autocompound {}),
            &[],
            address,
        )?;
    }

    Ok(resp.status)
}

mod utils {
    use cosmos_sdk_proto::{
        cosmos::base::query::v1beta1::{PageRequest, PageResponse},
        cosmwasm::wasm::v1::QueryContractsByCodeResponse,
    };
    use cw_asset::AssetBase;

    use super::*;
    const MIN_REWARD: (&str, Uint128) = ("uosmo", Uint128::new(100_000));

    pub fn next_page_request(page_response: PageResponse) -> PageRequest {
        PageRequest {
            key: page_response.next_key,
            offset: 0,
            limit: 0,
            count_total: false,
            reverse: false,
        }
    }

    /// Get the contract instances of a given code_id
    pub async fn fetch_instances(
        channel: Channel,
        code_id: u64,
        version: &str,
    ) -> anyhow::Result<Vec<String>> {
        let mut cw_querier = QueryClient::new(channel);

        let mut contract_addrs = vec![];
        let mut pagination = None;

        loop {
            let QueryContractsByCodeResponse {
                mut contracts,
                pagination: next_pagination,
            } = cw_querier
                .contracts_by_code(QueryContractsByCodeRequest {
                    code_id,
                    pagination,
                })
                .await?
                .into_inner();
            contract_addrs.append(&mut contracts);
            match next_pagination {
                Some(page_response) => pagination = Some(next_page_request(page_response)),
                // Done with pagination can return out all of the contracts
                None => {
                    log!(Level::Info, "Savings addrs({version}): {contract_addrs:?}");
                    break anyhow::Ok(contract_addrs);
                }
            }
        }
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
                .filter(|c| {
                    c.denom.eq(USDT_OSMOSIS_DENOM)
                        || c.denom.eq(USDC_OSMOSIS_DENOM)
                        || c.denom.eq(USDC_OSMOSIS_AXL_DENOM)
                })
                .map(|c| match c.denom.as_str() {
                    USDT_OSMOSIS_DENOM => ValuedCoin {
                        coin: c.clone(),
                        usd_value: Uint128::one(),
                    },
                    USDC_OSMOSIS_DENOM => ValuedCoin {
                        coin: c.clone(),
                        usd_value: Uint128::one(),
                    },
                    USDC_OSMOSIS_AXL_DENOM => ValuedCoin {
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

    pub fn enough_rewards(rewards: AssetBase<String>) -> bool {
        let gas_asset = match rewards.info {
            cw_asset::AssetInfoBase::Native(denom) => denom == MIN_REWARD.0,
            _ => false,
        };
        gas_asset && rewards.amount >= MIN_REWARD.1
    }
}
