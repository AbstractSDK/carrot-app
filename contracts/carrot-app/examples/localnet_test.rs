use abstract_app::objects::{
    module::ModuleInfo, namespace::ABSTRACT_NAMESPACE, AccountId, AssetEntry,
};
use abstract_client::{Application, Namespace};
use abstract_dex_adapter::{interface::DexAdapter, DEX_ADAPTER_ID};
use abstract_interface::{Abstract, VCQueryFns};
use abstract_sdk::core::ans_host::QueryMsgFns;
use cosmwasm_std::{coins, Decimal, Uint128, Uint64};
use cw_orch::{
    anyhow,
    contract::Deploy,
    daemon::{
        networks::{LOCAL_OSMO, OSMOSIS_1, OSMO_5},
        Daemon, DaemonBuilder,
    },
    prelude::*,
    tokio::runtime::Runtime,
};
use dotenv::dotenv;

use carrot_app::{
    autocompound::{AutocompoundConfigBase, AutocompoundRewardsConfigBase},
    contract::APP_ID,
    msg::AppInstantiateMsg,
    state::ConfigBase,
    yield_sources::{
        osmosis_cl_pool::ConcentratedPoolParamsBase, yield_type::YieldTypeBase, AssetShare,
        StrategyBase, StrategyElementBase, YieldSourceBase,
    },
    AppExecuteMsgFns, AppInterface,
};

pub const ION: &str = "uion";
pub const OSMO: &str = "uosmo";

pub const TICK_SPACING: u64 = 100;
pub const SPREAD_FACTOR: u64 = 0;

pub const INITIAL_LOWER_TICK: i64 = -100000;
pub const INITIAL_UPPER_TICK: i64 = 10000;

pub const POOL_ID: u64 = 2;
pub const USER_NAMESPACE: &str = "usernamespace";

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    let mut chain = LOCAL_OSMO;
    chain.grpc_urls = &["http://localhost:9090"];
    chain.chain_id = "osmosis-1";

    let rt = Runtime::new()?;
    let daemon = DaemonBuilder::default()
        .chain(chain)
        .handle(rt.handle())
        .build()?;

    let client = abstract_client::AbstractClient::new(daemon.clone())?;

    let block_info = daemon.block_info()?;

    // Verify modules exist
    let account = client
        .account_builder()
        .install_on_sub_account(false)
        .namespace(USER_NAMESPACE.try_into()?)
        .build()?;

    let carrot: Application<Daemon, carrot_app::AppInterface<Daemon>> = account.application()?;

    daemon.rt_handle.block_on(
        daemon
            .daemon
            .sender
            .bank_send(account.proxy()?.as_str(), coins(10_000, "uosmo")),
    )?;

    carrot.deposit(coins(10_000, "uosmo"), None)?;

    Ok(())
}
