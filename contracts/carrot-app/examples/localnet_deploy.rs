use abstract_app::objects::{
    namespace::ABSTRACT_NAMESPACE, pool_id::PoolAddressBase, AssetEntry, PoolMetadata, PoolType,
};
use abstract_client::{AbstractClient, Namespace};
use cosmwasm_std::Decimal;
use cw_asset::AssetInfoUnchecked;
use cw_orch::{
    anyhow,
    daemon::{networks::LOCAL_OSMO, DaemonBuilder},
    prelude::*,
    tokio::runtime::Runtime,
};
use dotenv::dotenv;

use cw_orch::osmosis_test_tube::osmosis_test_tube::cosmrs::proto::traits::Message;
use osmosis_std::types::{
    cosmos::base::v1beta1,
    osmosis::concentratedliquidity::{
        poolmodel::concentrated::v1beta1::{
            MsgCreateConcentratedPool, MsgCreateConcentratedPoolResponse,
        },
        v1beta1::{MsgCreatePosition, MsgCreatePositionResponse},
    },
};
use prost_types::Any;

pub const ION: &str = "uion";
pub const OSMO: &str = "uosmo";

pub const TICK_SPACING: u64 = 100;
pub const SPREAD_FACTOR: u64 = 0;

pub const INITIAL_LOWER_TICK: i64 = -100000;
pub const INITIAL_UPPER_TICK: i64 = 10000;

pub fn main() -> anyhow::Result<()> {
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

    // We create a CL pool
    let pool_id = create_pool(daemon.clone())?;
    // We register the ans entries of ion and osmosis balances
    register_ans(daemon.clone(), pool_id)?;

    deploy_app(daemon.clone())?;
    Ok(())
}

pub fn create_pool<Chain: CwEnv + Stargate>(chain: Chain) -> anyhow::Result<u64> {
    let response = chain.commit_any::<MsgCreateConcentratedPoolResponse>(
        vec![Any {
            value: MsgCreateConcentratedPool {
                sender: chain.sender().to_string(),
                denom0: ION.to_owned(),
                denom1: OSMO.to_owned(),
                tick_spacing: TICK_SPACING,
                spread_factor: SPREAD_FACTOR.to_string(),
            }
            .encode_to_vec(),
            type_url: MsgCreateConcentratedPool::TYPE_URL.to_string(),
        }],
        None,
    )?;

    let pool_id = response
        .event_attr_value("pool_created", "pool_id")?
        .parse()?;
    // Provide liquidity

    chain.commit_any::<MsgCreatePositionResponse>(
        vec![Any {
            type_url: MsgCreatePosition::TYPE_URL.to_string(),
            value: MsgCreatePosition {
                pool_id,
                sender: chain.sender().to_string(),
                lower_tick: INITIAL_LOWER_TICK,
                upper_tick: INITIAL_UPPER_TICK,
                tokens_provided: vec![
                    v1beta1::Coin {
                        denom: ION.to_string(),
                        amount: "1_000_000".to_owned(),
                    },
                    v1beta1::Coin {
                        denom: OSMO.to_string(),
                        amount: "1_000_000".to_owned(),
                    },
                ],
                token_min_amount0: "0".to_string(),
                token_min_amount1: "0".to_string(),
            }
            .encode_to_vec(),
        }],
        None,
    )?;
    Ok(pool_id)
}

pub fn register_ans<Chain: CwEnv>(chain: Chain, pool_id: u64) -> anyhow::Result<()> {
    let asset0 = ION.to_owned();
    let asset1 = OSMO.to_owned();
    // We register the pool inside the Abstract ANS
    let _client = AbstractClient::builder(chain.clone())
        .dex("osmosis")
        .assets(vec![
            (ION.to_string(), AssetInfoUnchecked::Native(asset0.clone())),
            (OSMO.to_string(), AssetInfoUnchecked::Native(asset1.clone())),
        ])
        .pools(vec![(
            PoolAddressBase::Id(pool_id),
            PoolMetadata {
                dex: "osmosis".to_owned(),
                pool_type: PoolType::ConcentratedLiquidity,
                assets: vec![AssetEntry::new(ION), AssetEntry::new(OSMO)],
            },
        )])
        .build()?;

    Ok(())
}

pub fn deploy_app<Chain: CwEnv>(chain: Chain) -> anyhow::Result<()> {
    let client = abstract_client::AbstractClient::new(chain.clone())?;
    // We deploy the carrot_app
    let publisher = client
        .publisher_builder(Namespace::new(ABSTRACT_NAMESPACE)?)
        .install_on_sub_account(false)
        .build()?;

    // The dex adapter
    publisher.publish_adapter::<_, abstract_dex_adapter::interface::DexAdapter<Chain>>(
        abstract_dex_adapter::msg::DexInstantiateMsg {
            swap_fee: Decimal::permille(2),
            recipient_account: 0,
        },
    )?;

    // The savings app
    publisher.publish_app::<carrot_app::contract::interface::AppInterface<Chain>>()?;

    Ok(())
}
