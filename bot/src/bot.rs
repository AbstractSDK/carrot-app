use abstract_client::{AbstractClient, AccountSource, Environment};
use carrot_app::{
    msg::{
        AppExecuteMsg, AppQueryMsg, CompoundStatus, CompoundStatusResponse, ExecuteMsg, QueryMsg,
    },
    AppInterface,
};
use cw_asset::AssetInfo;
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
    collections::{HashMap, HashSet},
    fmt::{Display, Formatter},
    time::{Duration, SystemTime},
};
use tonic::transport::Channel;

use abstract_app::{
    abstract_interface::VCQueryFns,
    objects::module::{ModuleInfo, ModuleStatus},
    std::{ans_host, version_control::ModuleFilter},
};

const LAST_INCOMPATIBLE_VERSION: &str = "0.3.1";
const VERSION_REQ: &str = ">=0.4, <0.6";

const AUTHORIZATION_URLS: &[&str] = &[
    MsgCreatePosition::TYPE_URL,
    MsgSwapExactAmountIn::TYPE_URL,
    MsgAddToPosition::TYPE_URL,
    MsgWithdrawPosition::TYPE_URL,
    MsgCollectIncentives::TYPE_URL,
    MsgCollectSpreadRewards::TYPE_URL,
];

const USD_ASSETS: [&str; 3] = ["eth>axelar>usdc", "noble>usdc", "kava>usdt"];

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
    // Resolved assets and their value
    assets_value: HashMap<AssetInfo, Uint128>,
}

#[derive(Eq, Hash, PartialEq, Clone)]
struct CarrotInstance {
    address: Addr,
    version: String,
}
impl CarrotInstance {
    fn new(address: impl Into<String>, version: &str) -> Self {
        Self {
            address: Addr::unchecked(address),
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
            assets_value: Default::default(),
        }
    }

    // Fetches contracts and assets if fetch cooldown passed
    pub fn fetch_contracts_and_assets(&mut self) -> anyhow::Result<()> {
        // Don't fetch if not ready
        let ready_time = self.last_fetch + self.fetch_contracts_cooldown;
        if SystemTime::now() < ready_time {
            return Ok(());
        }

        log!(Level::Info, "Fetching contracts and assets");

        let daemon = &self.daemon;

        let abstr = AbstractClient::new(self.daemon.clone())?;
        // Refresh asset values
        log!(Level::Debug, "Fetching assets");
        self.assets_value = {
            let names = USD_ASSETS
                .into_iter()
                .map(ToOwned::to_owned)
                .collect::<Vec<String>>();
            let assets_response: ans_host::AssetsResponse = abstr
                .name_service()
                .query(&ans_host::QueryMsg::Assets { names })?;
            assets_response
                .assets
                .into_iter()
                // For now we just assume 1 usd asset == 1 usd
                .map(|(_, info)| (info, Uint128::one()))
                .collect()
        };

        let mut contract_instances_to_autocompound: HashSet<(String, CarrotInstance)> =
            HashSet::new();
        let mut contract_instances_to_skip: HashSet<CarrotInstance> = HashSet::new();

        log!(Level::Debug, "Fetching modules");
        let saving_modules = utils::carrot_module_list(&abstr, &self.module_info)?;

        let mut fetch_instances_count = 0;

        let ver_req = VersionReq::parse(VERSION_REQ).unwrap();
        log!(Level::Debug, "version requirement: {ver_req}");
        for app_info in saving_modules.modules {
            let version = app_info.module.info.version.to_string();
            // Completely ignore outdated carrots
            if !semver::Version::parse(&version)
                .map(|v| ver_req.matches(&v))
                .unwrap_or(false)
            {
                continue;
            }

            let code_id = app_info.module.reference.unwrap_app()?;

            let contract_addrs = daemon.rt_handle.block_on(utils::fetch_instances(
                daemon.channel(),
                code_id,
                &version,
            ))?;
            fetch_instances_count += contract_addrs.len();

            // Need to reset contract balances in case some of the contracts migrated
            self.metrics.contract_balance.reset();
            for contract_addr in contract_addrs.iter() {
                let address = Addr::unchecked(contract_addr.clone());
                let balance_result =
                    utils::get_carrot_balance(daemon.clone(), &self.assets_value, &address);
                let balance = match balance_result {
                    Ok(value) => {
                        if !value.is_zero() {
                            log!(Level::Info, "contract: {contract_addr} balance: {value}");
                        }
                        value
                    }
                    Err(e) => {
                        log!(Level::Error, "contract: {contract_addr} err:{e:?}");
                        Uint128::zero()
                    }
                };

                // Update contract_balance GaugeVec with label
                let label = labels! {"contract_address"=> contract_addr.as_ref(),"contract_version"=> version.as_ref()};

                self.metrics
                    .contract_balance
                    .with(&label)
                    .set(balance.u128().try_into().unwrap());

                // Insert instances that are supposed to be autocompounded
                if !balance.is_zero()
                    && utils::has_authz_permission(&abstr, contract_addr).unwrap_or(false)
                {
                    contract_instances_to_autocompound.insert((
                        app_info.module.info.id(),
                        CarrotInstance::new(contract_addr, version.as_ref()),
                    ));
                } else {
                    contract_instances_to_skip
                        .insert(CarrotInstance::new(contract_addr, version.as_ref()));
                }
            }
        }

        self.contract_instances_to_ac = contract_instances_to_autocompound;

        log!(
            Level::Info,
            "Skipping instances: {}",
            contract_instances_to_skip
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<String>>()
                .join(",")
        );
        // Metrics
        self.metrics.fetch_count.inc();
        self.metrics
            .fetch_instances_count
            .set(fetch_instances_count as i64);
        self.metrics
            .contract_instances_to_autocompound
            .set(self.contract_instances_to_ac.len() as i64);

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
    use abstract_app::std::version_control::ModulesListResponse;
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
                // `next_key` can still be empty, meaning there are no next key
                Some(page_response) if !page_response.next_key.is_empty() => {
                    pagination = Some(next_page_request(page_response))
                }
                // Done with pagination can return out all of the contracts
                _ => {
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
    pub fn get_carrot_balance(
        daemon: Daemon,
        assets_values: &HashMap<AssetInfo, Uint128>,
        contract_addr: &Addr,
    ) -> anyhow::Result<Uint128> {
        let response: carrot_app::msg::AssetsBalanceResponse =
            daemon.query(&QueryMsg::from(AppQueryMsg::Balance {}), contract_addr)?;

        let balance = Balance::new(
            response
                .balances
                .into_iter()
                .map(|coin| ValuedCoin {
                    usd_value: assets_values
                        .get(&AssetInfo::native(coin.denom.clone()))
                        .map(ToOwned::to_owned)
                        .unwrap_or(Uint128::zero()),
                    coin,
                })
                .collect(),
        );
        let balance = balance.calculate_usd_value();

        Ok(balance)
    }

    pub fn enough_rewards(rewards: AssetBase<String>) -> bool {
        let gas_asset = match rewards.info {
            cw_asset::AssetInfoBase::Native(denom) => denom == MIN_REWARD.0,
            _ => false,
        };
        gas_asset && rewards.amount >= MIN_REWARD.1
    }

    pub fn carrot_module_list(
        abstr: &AbstractClient<Daemon>,
        module_info: &ModuleInfo,
    ) -> Result<ModulesListResponse, cw_orch::core::CwEnvError> {
        let mut start_after = Some(ModuleInfo {
            namespace: module_info.namespace.clone(),
            name: module_info.name.clone(),
            version: abstract_app::objects::module::ModuleVersion::Version(
                LAST_INCOMPATIBLE_VERSION.to_owned(),
            ),
        });
        let mut module_list = ModulesListResponse { modules: vec![] };
        loop {
            let saving_modules = abstr.version_control().module_list(
                Some(ModuleFilter {
                    namespace: Some(module_info.namespace.to_string()),
                    name: Some(module_info.name.clone()),
                    version: None,
                    status: Some(ModuleStatus::Registered),
                }),
                None,
                start_after,
            )?;
            if saving_modules.modules.is_empty() {
                break;
            }
            start_after = saving_modules
                .modules
                .last()
                .map(|mod_respose| mod_respose.module.info.clone());
            module_list.modules.extend(saving_modules.modules);
        }
        Ok(module_list)
    }
}
