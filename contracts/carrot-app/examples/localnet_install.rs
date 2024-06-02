use abstract_client::Application;
use abstract_dex_adapter::interface::DexAdapter;
use cosmwasm_std::{Decimal, Uint64};
use cw_orch::{
    anyhow,
    daemon::{networks::LOCAL_OSMO, Daemon, DaemonBuilder},
    prelude::*,
    tokio::runtime::Runtime,
};
use dotenv::dotenv;

use carrot_app::{
    autocompound::{AutocompoundConfigBase, AutocompoundRewardsConfigBase},
    msg::AppInstantiateMsg,
    state::ConfigBase,
    yield_sources::{
        osmosis_cl_pool::ConcentratedPoolParamsBase, yield_type::YieldParamsBase, AssetShare,
        StrategyBase, StrategyElementBase, StrategyElementUnchecked, StrategyUnchecked,
        YieldSourceBase,
    },
};

pub const ION: &str = "uion";
pub const OSMO: &str = "uosmo";

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
                    reward_percent: Decimal::percent(10),
                    _phantom: std::marker::PhantomData,
                },
            },
            dex: "osmosis".to_string(),
        },
        strategy: two_strategy(),
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

fn one_element(upper_tick: i64, lower_tick: i64, share: Decimal) -> StrategyElementUnchecked {
    StrategyElementBase {
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
                lower_tick,
                upper_tick,
                position_id: None,
                _phantom: std::marker::PhantomData,
                position_cache: None,
            }),
        },
        share,
    }
}

pub fn single_strategy() -> StrategyUnchecked {
    StrategyBase(vec![one_element(
        INITIAL_UPPER_TICK,
        INITIAL_LOWER_TICK,
        Decimal::one(),
    )])
}

pub fn two_strategy() -> StrategyUnchecked {
    StrategyBase(vec![
        one_element(INITIAL_UPPER_TICK, INITIAL_LOWER_TICK, Decimal::percent(50)),
        one_element(5000, -5000, Decimal::percent(50)),
    ])
}

pub fn three_strategy() -> StrategyUnchecked {
    StrategyBase(vec![
        one_element(INITIAL_UPPER_TICK, INITIAL_LOWER_TICK, Decimal::percent(33)),
        one_element(5000, -5000, Decimal::percent(33)),
        one_element(1000, -1000, Decimal::percent(34)),
    ])
}

pub fn four_strategy() -> StrategyUnchecked {
    StrategyBase(vec![
        one_element(INITIAL_UPPER_TICK, INITIAL_LOWER_TICK, Decimal::percent(25)),
        one_element(5000, -5000, Decimal::percent(25)),
        one_element(1000, -1000, Decimal::percent(25)),
        one_element(100, -100, Decimal::percent(25)),
    ])
}

pub fn five_strategy() -> StrategyUnchecked {
    StrategyBase(vec![
        one_element(INITIAL_UPPER_TICK, INITIAL_LOWER_TICK, Decimal::percent(20)),
        one_element(5000, -5000, Decimal::percent(20)),
        one_element(1000, -1000, Decimal::percent(20)),
        one_element(100, -100, Decimal::percent(20)),
        one_element(600, -600, Decimal::percent(20)),
    ])
}
