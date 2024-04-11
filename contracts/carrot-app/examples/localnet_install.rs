use abstract_app::objects::{
    module::ModuleInfo, namespace::ABSTRACT_NAMESPACE, AccountId, AssetEntry,
};
use abstract_client::{Application, Namespace};
use abstract_dex_adapter::{interface::DexAdapter, DEX_ADAPTER_ID};
use abstract_interface::{Abstract, VCQueryFns};
use abstract_sdk::core::{ans_host::QueryMsgFns, app::BaseMigrateMsg};
use cosmwasm_std::{Decimal, Uint128, Uint64};
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
    msg::{AppInstantiateMsg, AppMigrateMsg, MigrateMsg},
    state::ConfigBase,
    yield_sources::{
        osmosis_cl_pool::ConcentratedPoolParamsBase, yield_type::YieldParamsBase, AssetShare,
        StrategyBase, StrategyElementBase, YieldSourceBase,
    },
    AppInterface,
};

pub const ION: &str = "uion";
pub const OSMO: &str = "uosmo";

pub const TICK_SPACING: u64 = 100;
pub const SPREAD_FACTOR: u64 = 0;

pub const INITIAL_LOWER_TICK: i64 = -100000;
pub const INITIAL_UPPER_TICK: i64 = 10000;

pub const POOL_ID: u64 = 1;
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

    // Verify modules exist
    let account = client
        .account_builder()
        .install_on_sub_account(false)
        .namespace(USER_NAMESPACE.try_into()?)
        .build()?;

    let init_msg = AppInstantiateMsg {
        config: ConfigBase {
            // 5 mins
            autocompound_config: AutocompoundConfigBase {
                cooldown_seconds: Uint64::new(300),
                rewards: AutocompoundRewardsConfigBase {
                    gas_asset: AssetEntry::new(OSMO),
                    swap_asset: AssetEntry::new(ION),
                    reward: Uint128::new(1000),
                    min_gas_balance: Uint128::new(2000),
                    max_gas_balance: Uint128::new(10000),
                    _phantom: std::marker::PhantomData,
                },
            },
            dex: "osmosis".to_string(),
        },
        strategy: StrategyBase(vec![StrategyElementBase {
            yield_source: YieldSourceBase {
                asset_distribution: vec![
                    AssetShare {
                        denom: ION.to_string(),
                        share: Decimal::percent(50),
                    },
                    AssetShare {
                        denom: OSMO.to_string(),
                        share: Decimal::percent(50),
                    },
                ],
                params: YieldParamsBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                    pool_id: POOL_ID,
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                    _phantom: std::marker::PhantomData,
                }),
            },
            share: Decimal::one(),
        }]),
        deposit: None,
    };

    let carrot_app = account
        .install_app_with_dependencies::<carrot_app::contract::interface::AppInterface<Daemon>>(
            &init_msg,
            Empty {},
            &[],
        )?;

    // We update authorized addresses on the adapter for the app
    let dex_adapter: Application<Daemon, DexAdapter<Daemon>> = account.application()?;
    dex_adapter.execute(
        &abstract_dex_adapter::msg::ExecuteMsg::Base(
            abstract_app::abstract_core::adapter::BaseExecuteMsg {
                proxy_address: Some(carrot_app.account().proxy()?.to_string()),
                msg: abstract_app::abstract_core::adapter::AdapterBaseMsg::UpdateAuthorizedAddresses {
                    to_add: vec![carrot_app.addr_str()?],
                    to_remove: vec![],
                },
            },
        ),
        None,
    )?;

    Ok(())
}
