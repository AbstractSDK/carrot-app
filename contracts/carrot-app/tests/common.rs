use abstract_app::abstract_core::objects::{
    pool_id::PoolAddressBase, AssetEntry, PoolMetadata, PoolType,
};
use abstract_client::{AbstractClient, Application, Namespace};
use carrot_app::autocompound::{AutocompoundConfigBase, AutocompoundRewardsConfigBase};
use carrot_app::contract::OSMOSIS;
use carrot_app::msg::AppInstantiateMsg;
use carrot_app::state::ConfigBase;
use carrot_app::yield_sources::osmosis_cl_pool::ConcentratedPoolParamsBase;
use carrot_app::yield_sources::yield_type::YieldTypeBase;
use carrot_app::yield_sources::{
    AssetShare, BalanceStrategyBase, BalanceStrategyElementBase, YieldSourceBase,
};
use cosmwasm_std::{coin, coins, Coins, Decimal, Uint128, Uint64};
use cw_asset::AssetInfoUnchecked;
use cw_orch::environment::MutCwEnv;
use cw_orch::osmosis_test_tube::osmosis_test_tube::Gamm;
use cw_orch::{
    anyhow,
    osmosis_test_tube::osmosis_test_tube::{
        osmosis_std::types::{
            cosmos::base::v1beta1,
            osmosis::concentratedliquidity::v1beta1::{MsgCreatePosition, Pool, PoolsRequest},
        },
        ConcentratedLiquidity, GovWithAppAccess, Module,
    },
    prelude::*,
};
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    CreateConcentratedLiquidityPoolsProposal, PoolRecord,
};
use prost::Message;
pub const LOTS: u128 = 100_000_000_000_000;

// Asset 0
pub const USDT: &str = "ibc/4ABBEF4C8926DDDB320AE5188CFD63267ABBCEFC0583E4AE05D6E5AA2401DDAB";

// Asset 1
pub const USDC: &str = "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";

pub const REWARD_DENOM: &str = "reward";
pub const REWARD_ASSET: &str = "rew";
pub const GAS_DENOM: &str = "uosmo";
pub const DEX_NAME: &str = "osmosis";

pub const TICK_SPACING: u64 = 100;
pub const SPREAD_FACTOR: u64 = 1;

pub const INITIAL_LOWER_TICK: i64 = -100000;
pub const INITIAL_UPPER_TICK: i64 = 10000;
// Deploys abstract and other contracts
pub fn deploy<Chain: MutCwEnv + Stargate>(
    mut chain: Chain,
    pool_id: u64,
    gas_pool_id: u64,
    initial_deposit: Option<Vec<Coin>>,
) -> anyhow::Result<Application<Chain, carrot_app::AppInterface<Chain>>> {
    let asset0 = USDT.to_owned();
    let asset1 = USDC.to_owned();
    // We register the pool inside the Abstract ANS
    let client = AbstractClient::builder(chain.clone())
        .dex(DEX_NAME)
        .assets(vec![
            (USDC.to_string(), AssetInfoUnchecked::Native(asset0.clone())),
            (USDT.to_string(), AssetInfoUnchecked::Native(asset1.clone())),
            (
                REWARD_ASSET.to_string(),
                AssetInfoUnchecked::Native(REWARD_DENOM.to_owned()),
            ),
        ])
        .pools(vec![
            (
                PoolAddressBase::Id(gas_pool_id),
                PoolMetadata {
                    dex: DEX_NAME.to_owned(),
                    pool_type: PoolType::ConcentratedLiquidity,
                    assets: vec![AssetEntry::new(USDC), AssetEntry::new(REWARD_ASSET)],
                },
            ),
            (
                PoolAddressBase::Id(pool_id),
                PoolMetadata {
                    dex: DEX_NAME.to_owned(),
                    pool_type: PoolType::ConcentratedLiquidity,
                    assets: vec![AssetEntry::new(USDC), AssetEntry::new(USDT)],
                },
            ),
        ])
        .build()?;

    // We deploy the carrot_app
    let publisher = client
        .publisher_builder(Namespace::new("abstract")?)
        .install_on_sub_account(false)
        .build()?;
    // The dex adapter
    let dex_adapter = publisher
        .publish_adapter::<_, abstract_dex_adapter::interface::DexAdapter<Chain>>(
            abstract_dex_adapter::msg::DexInstantiateMsg {
                swap_fee: Decimal::permille(2),
                recipient_account: 0,
            },
        )?;
    // // The moneymarket adapter
    // let money_market_adapter = publisher
    //     .publish_adapter::<_, abstract_money_market_adapter::interface::MoneyMarketAdapter<
    //     Chain,
    // >>(
    //     abstract_money_market_adapter::msg::MoneyMarketInstantiateMsg {
    //         fee: Decimal::percent(2),
    //         recipient_account: 0,
    //     },
    // )?;
    // The savings app
    publisher.publish_app::<carrot_app::contract::interface::AppInterface<Chain>>()?;

    if let Some(deposit) = &initial_deposit {
        chain.add_balance(publisher.account().proxy()?.to_string(), deposit.clone())?;
    }

    let init_msg = AppInstantiateMsg {
        config: ConfigBase {
            // 5 mins
            autocompound_config: AutocompoundConfigBase {
                cooldown_seconds: Uint64::new(300),
                rewards: AutocompoundRewardsConfigBase {
                    gas_asset: AssetEntry::new(REWARD_ASSET),
                    swap_asset: AssetEntry::new(USDC),
                    reward: Uint128::new(1000),
                    min_gas_balance: Uint128::new(2000),
                    max_gas_balance: Uint128::new(10000),
                    _phantom: std::marker::PhantomData,
                },
            },
            balance_strategy: BalanceStrategyBase(vec![BalanceStrategyElementBase {
                yield_source: YieldSourceBase {
                    asset_distribution: vec![
                        AssetShare {
                            denom: USDT.to_string(),
                            share: Decimal::percent(50),
                        },
                        AssetShare {
                            denom: USDC.to_string(),
                            share: Decimal::percent(50),
                        },
                    ],
                    ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                        pool_id,
                        lower_tick: INITIAL_LOWER_TICK,
                        upper_tick: INITIAL_UPPER_TICK,
                        position_id: None,
                        _phantom: std::marker::PhantomData,
                    }),
                },
                share: Decimal::one(),
            }]),
            dex: OSMOSIS.to_string(),
        },
        deposit: initial_deposit,
    };

    // We install the carrot-app
    let carrot_app: Application<Chain, carrot_app::AppInterface<Chain>> =
        publisher
            .account()
            .install_app_with_dependencies::<carrot_app::contract::interface::AppInterface<Chain>>(
                &init_msg,
                Empty {},
                &[],
            )?;
    // We update authorized addresses on the adapter for the app
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
    // money_market_adapter.execute(
    //     &abstract_money_market_adapter::msg::ExecuteMsg::Base(
    //         abstract_app::abstract_core::adapter::BaseExecuteMsg {
    //             proxy_address: Some(carrot_app.account().proxy()?.to_string()),
    //             msg: abstract_app::abstract_core::adapter::AdapterBaseMsg::UpdateAuthorizedAddresses {
    //                 to_add: vec![carrot_app.addr_str()?],
    //                 to_remove: vec![],
    //             },
    //         },
    //     ),
    //     None,
    // )?;

    Ok(carrot_app)
}

pub fn create_pool(mut chain: OsmosisTestTube) -> anyhow::Result<(u64, u64)> {
    chain.add_balance(chain.sender(), coins(LOTS, USDC))?;
    chain.add_balance(chain.sender(), coins(LOTS, USDT))?;

    let asset0 = USDT.to_owned();
    let asset1 = USDC.to_owned();
    // Message for an actual chain (creating concentrated pool)
    // let create_pool_response = chain.commit_any::<MsgCreateConcentratedPoolResponse>(
    //     vec![Any {
    //         value: MsgCreateConcentratedPool {
    //             sender: chain.sender().to_string(),
    //             denom0: USDT.to_owned(),
    //             denom1: USDC.to_owned(),
    //             tick_spacing: TICK_SPACING,
    //             spread_factor: SPREAD_FACTOR.to_string(),
    //         }
    //         .encode_to_vec(),
    //         type_url: MsgCreateConcentratedPool::TYPE_URL.to_string(),
    //     }],
    //     None,
    // )?;
    GovWithAppAccess::new(&chain.app.borrow())
        .propose_and_execute(
            CreateConcentratedLiquidityPoolsProposal::TYPE_URL.to_string(),
            CreateConcentratedLiquidityPoolsProposal {
                title: "Create concentrated uosmo:usdc pool".to_string(),
                description: "Create concentrated uosmo:usdc pool, so that we can trade it"
                    .to_string(),
                pool_records: vec![PoolRecord {
                    denom0: USDT.to_owned(),
                    denom1: USDC.to_owned(),
                    tick_spacing: TICK_SPACING,
                    spread_factor: Decimal::percent(SPREAD_FACTOR).atomics().to_string(),
                }],
            },
            chain.sender().to_string(),
            &chain.sender,
        )
        .unwrap();

    let test_tube = chain.app.borrow();
    let cl = ConcentratedLiquidity::new(&*test_tube);

    let pools = cl.query_pools(&PoolsRequest { pagination: None }).unwrap();

    let pool = Pool::decode(pools.pools.last().unwrap().value.as_slice()).unwrap();
    let _response = cl
        .create_position(
            MsgCreatePosition {
                pool_id: pool.id,
                sender: chain.sender().to_string(),
                lower_tick: INITIAL_LOWER_TICK,
                upper_tick: INITIAL_UPPER_TICK,
                tokens_provided: vec![
                    v1beta1::Coin {
                        denom: asset1,
                        amount: "1_000_000".to_owned(),
                    },
                    v1beta1::Coin {
                        denom: asset0.clone(),
                        amount: "1_000_000".to_owned(),
                    },
                ],
                token_min_amount0: "0".to_string(),
                token_min_amount1: "0".to_string(),
            },
            &chain.sender,
        )?
        .data;

    let gamm = Gamm::new(&*test_tube);
    let rewards_pool_provider = test_tube.init_account(&[
        Coin::new(1_000_000_000, asset0.clone()),
        Coin::new(2_000_000_000, REWARD_DENOM),
        Coin::new(LOTS, GAS_DENOM),
    ])?;

    let gas_pool_response = gamm.create_basic_pool(
        &[
            Coin::new(1_000_000_000, asset0),
            Coin::new(2_000_000_000, REWARD_DENOM),
        ],
        &rewards_pool_provider,
    )?;

    Ok((pool.id, gas_pool_response.data.pool_id))
}

pub fn setup_test_tube(
    create_position: bool,
) -> anyhow::Result<(
    u64,
    Application<OsmosisTestTube, carrot_app::AppInterface<OsmosisTestTube>>,
)> {
    let _ = env_logger::builder().is_test(true).try_init();
    let chain = OsmosisTestTube::new(vec![coin(LOTS, GAS_DENOM)]);

    // We create a usdt-usdc pool
    let (pool_id, gas_pool_id) = create_pool(chain.clone())?;

    let initial_deposit: Option<Vec<Coin>> = create_position
        .then(|| {
            // TODO: Requires instantiate2 to test it (we need to give authz authorization before instantiating)
            let mut initial_coins = Coins::default();
            initial_coins.add(coin(10_000, USDT))?;
            initial_coins.add(coin(10_000, USDC))?;
            Ok::<_, anyhow::Error>(initial_coins.into())
        })
        .transpose()?;
    let carrot_app = deploy(chain.clone(), pool_id, gas_pool_id, initial_deposit)?;

    Ok((pool_id, carrot_app))
}
