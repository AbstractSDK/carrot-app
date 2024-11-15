use std::iter;

use abstract_app::objects::module::ModuleInfo;
use abstract_app::objects::namespace::ABSTRACT_NAMESPACE;
use abstract_app::std::{
    account::{self, ModuleInstallConfig},
    objects::{pool_id::PoolAddressBase, AccountId, AssetEntry, PoolMetadata, PoolType},
};
use abstract_client::{AbstractClient, Application, Environment, Namespace};
use abstract_dex_adapter::DEX_ADAPTER_ID;
use carrot_app::contract::APP_ID;
use carrot_app::msg::{AppInstantiateMsg, CreatePositionMessage};
use carrot_app::state::AutocompoundRewardsConfig;
use cosmwasm_std::{coin, coins, to_json_binary, to_json_vec, Decimal, Uint128, Uint64};
use cw_asset::AssetInfoUnchecked;
use cw_orch::anyhow;
use cw_orch::prelude::*;
use cw_orch_osmosis_test_tube::osmosis_test_tube::osmosis_std::types::{
    cosmos::bank::v1beta1::SendAuthorization,
    cosmwasm::wasm::v1::MsgExecuteContract,
    osmosis::concentratedliquidity::v1beta1::{
        CreateConcentratedLiquidityPoolsProposal, MsgAddToPosition, MsgCollectIncentives,
        MsgCollectSpreadRewards, PoolRecord,
    },
};
use cw_orch_osmosis_test_tube::osmosis_test_tube::{
    osmosis_std::types::{
        cosmos::{
            authz::v1beta1::{GenericAuthorization, Grant, MsgGrant},
            base::v1beta1,
        },
        osmosis::{
            concentratedliquidity::v1beta1::{
                MsgCreatePosition, MsgWithdrawPosition, Pool, PoolsRequest,
            },
            gamm::v1beta1::MsgSwapExactAmountIn,
        },
    },
    ConcentratedLiquidity, Gamm, GovWithAppAccess, Module,
};
use cw_orch_osmosis_test_tube::OsmosisTestTube;
use prost::Message;
use prost_types::Any;

pub const LOTS: u128 = 100_000_000_000_000;

// Asset 0
pub const USDT: &str = "USDT";
pub const USDT_DENOM: &str = "ibc/4ABBEF4C8926DDDB320AE5188CFD63267ABBCEFC0583E4AE05D6E5AA2401DDAB";

// Asset 1
pub const USDC: &str = "USDC";
pub const USDC_DENOM: &str = "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";

pub const REWARD_DENOM: &str = "reward";
pub const REWARD_ASSET: &str = "rew";
pub const GAS_DENOM: &str = "uosmo";
pub const DEX_NAME: &str = "osmosis";

pub const TICK_SPACING: u64 = 100;
pub const SPREAD_FACTOR: u64 = 1;

pub const INITIAL_LOWER_TICK: i64 = -100000;
pub const INITIAL_UPPER_TICK: i64 = 10000;
// Deploys abstract and other contracts
pub fn deploy<Chain: CwEnv + Stargate>(
    chain: Chain,
    pool_id: u64,
    gas_pool_id: u64,
    create_position: Option<CreatePositionMessage>,
) -> anyhow::Result<Application<Chain, carrot_app::AppInterface<Chain>>> {
    let asset0 = USDT_DENOM.to_owned();
    let asset1 = USDC_DENOM.to_owned();
    // We register the pool inside the Abstract ANS
    let client = AbstractClient::builder(chain.clone())
        .dex(DEX_NAME)
        .assets(vec![
            (USDT.to_string(), AssetInfoUnchecked::Native(asset0.clone())),
            (USDC.to_string(), AssetInfoUnchecked::Native(asset1.clone())),
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
        .fetch_or_build_account(Namespace::new(ABSTRACT_NAMESPACE)?, |builder| {
            builder.namespace(Namespace::new(ABSTRACT_NAMESPACE).unwrap())
        })?
        .publisher()?;
    // The dex adapter
    let dex_adapter = publisher
        .publish_adapter::<_, abstract_dex_adapter::interface::DexAdapter<Chain>>(
            abstract_dex_adapter::msg::DexInstantiateMsg {
                swap_fee: Decimal::percent(2),
                recipient_account: 0,
            },
        )?;
    // The savings app
    publisher.publish_app::<carrot_app::contract::interface::AppInterface<Chain>>()?;

    let create_position_on_init = create_position.is_some();
    let init_msg = AppInstantiateMsg {
        pool_id,
        // 5 mins
        autocompound_cooldown_seconds: Uint64::new(300),
        autocompound_rewards_config: AutocompoundRewardsConfig {
            gas_asset: AssetEntry::new(REWARD_ASSET),
            swap_asset: AssetEntry::new(USDC),
            reward: Uint128::new(1000),
            min_gas_balance: Uint128::new(2000),
            max_gas_balance: Uint128::new(10000),
        },
        create_position,
    };
    // If we create position on instantiate - give auth
    let carrot_app = if create_position_on_init {
        let random_account_id = client.random_account_id()?;

        let savings_app_addr = client
            .module_instantiate2_address::<carrot_app::AppInterface<Chain>>(&AccountId::local(
                random_account_id,
            ))?;

        // Give all authzs and create subaccount with app in single tx
        let mut msgs = give_authorizations_msgs(&client, savings_app_addr)?;
        let create_sub_account_message = Any {
            type_url: MsgExecuteContract::TYPE_URL.to_owned(),
            value: MsgExecuteContract {
                sender: chain.sender_addr().to_string(),
                contract: publisher.account().address()?.to_string(),
                msg: to_json_vec(&account::ExecuteMsg::<Empty>::CreateSubAccount {
                    name: Some("bob".to_owned()),
                    description: None,
                    link: None,
                    namespace: None,
                    install_modules: vec![
                        ModuleInstallConfig::new(ModuleInfo::from_id_latest(DEX_ADAPTER_ID)?, None),
                        ModuleInstallConfig::new(
                            ModuleInfo::from_id_latest(APP_ID)?,
                            Some(to_json_binary(&init_msg)?),
                        ),
                    ],
                    account_id: Some(random_account_id),
                })?,
                funds: vec![],
            }
            .to_proto_bytes(),
        };
        msgs.push(create_sub_account_message);
        let _ = chain.commit_any(msgs, None)?;

        // Now get Application struct
        let account = client.account_from(AccountId::local(random_account_id))?;
        account.application::<carrot_app::AppInterface<Chain>>()?
    } else {
        // We install the carrot-app
        let carrot_app: Application<Chain, carrot_app::AppInterface<Chain>> =
        publisher
            .account()
            .install_app_with_dependencies::<carrot_app::contract::interface::AppInterface<Chain>>(
                &init_msg,
                Empty {},
                &[],
            )?;
        carrot_app
    };
    // We update authorized addresses on the adapter for the app
    dex_adapter.execute(
        &abstract_dex_adapter::msg::ExecuteMsg::Base(abstract_app::std::adapter::BaseExecuteMsg {
            account_address: Some(carrot_app.account().address()?.to_string()),
            msg: abstract_app::std::adapter::AdapterBaseMsg::UpdateAuthorizedAddresses {
                to_add: vec![carrot_app.addr_str()?],
                to_remove: vec![],
            },
        }),
        &[],
    )?;

    Ok(carrot_app)
}

pub fn create_position<Chain: CwEnv>(
    app: &Application<Chain, carrot_app::AppInterface<Chain>>,
    funds: Vec<Coin>,
    asset0: Coin,
    asset1: Coin,
) -> anyhow::Result<Chain::Response> {
    app.execute(
        &carrot_app::msg::AppExecuteMsg::CreatePosition(CreatePositionMessage {
            lower_tick: INITIAL_LOWER_TICK,
            upper_tick: INITIAL_UPPER_TICK,
            funds,
            asset0,
            asset1,
            max_spread: None,
            belief_price0: None,
            belief_price1: None,
        })
        .into(),
        &[],
    )
    .map_err(Into::into)
}

pub fn create_pool(mut chain: OsmosisTestTube) -> anyhow::Result<(u64, u64)> {
    chain.add_balance(&chain.sender_addr(), coins(LOTS, USDC_DENOM))?;
    chain.add_balance(&chain.sender_addr(), coins(LOTS, USDT_DENOM))?;

    let asset0 = USDT_DENOM.to_owned();
    let asset1 = USDC_DENOM.to_owned();
    // Message for an actual chain (creating concentrated pool)
    // let create_pool_response = chain.commit_any::<MsgCreateConcentratedPoolResponse>(
    //     vec![Any {
    //         value: MsgCreateConcentratedPool {
    //             sender: chain.sender_addr().to_string(),
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
    let _proposal_response = GovWithAppAccess::new(&chain.app.borrow())
        .propose_and_execute(
            CreateConcentratedLiquidityPoolsProposal::TYPE_URL.to_string(),
            CreateConcentratedLiquidityPoolsProposal {
                title: "Create concentrated uosmo:usdc pool".to_string(),
                description: "Create concentrated uosmo:usdc pool, so that we can trade it"
                    .to_string(),
                #[allow(deprecated)]
                pool_records: vec![PoolRecord {
                    denom0: USDT_DENOM.to_owned(),
                    denom1: USDC_DENOM.to_owned(),
                    tick_spacing: TICK_SPACING,
                    spread_factor: Decimal::percent(SPREAD_FACTOR).atomics().to_string(),
                    exponent_at_price_one: String::default(),
                }],
            },
            chain.sender_addr().to_string(),
            &chain.sender,
        )
        .unwrap();
    let test_tube = chain.app.borrow();
    let cl = ConcentratedLiquidity::new(&*test_tube);

    let pools = cl.query_pools(&PoolsRequest { pagination: None }).unwrap();

    let pool = Pool::decode(pools.pools[0].value.as_slice()).unwrap();
    let _response = cl
        .create_position(
            MsgCreatePosition {
                pool_id: pool.id,
                sender: chain.sender_addr().to_string(),
                lower_tick: INITIAL_LOWER_TICK,
                upper_tick: INITIAL_UPPER_TICK,
                tokens_provided: vec![
                    v1beta1::Coin {
                        denom: asset1.clone(),
                        amount: "10_000_000".to_owned(),
                    },
                    v1beta1::Coin {
                        denom: asset0.clone(),
                        amount: "10_000_000".to_owned(),
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
        Coin::new(1_000_000_000_u128, asset1.clone()),
        Coin::new(2_000_000_000_u128, REWARD_DENOM),
        Coin::new(LOTS, GAS_DENOM),
    ])?;

    let gas_pool_response = gamm.create_basic_pool(
        &[
            Coin::new(1_000_000_000_u128, asset1),
            Coin::new(2_000_000_000_u128, REWARD_DENOM),
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
    let chain = OsmosisTestTube::new(coins(LOTS, GAS_DENOM));

    // We create a usdt-usdc pool
    let (pool_id, gas_pool_id) = create_pool(chain.clone())?;

    let create_position_msg = create_position.then(|| CreatePositionMessage {
        lower_tick: INITIAL_LOWER_TICK,
        upper_tick: INITIAL_UPPER_TICK,
        funds: coins(100_000, USDT_DENOM),
        asset0: coin(1_000_000, USDT_DENOM),
        asset1: coin(1_000_000, USDC_DENOM),
        max_spread: None,
        belief_price0: None,
        belief_price1: None,
    });
    let carrot_app = deploy(chain.clone(), pool_id, gas_pool_id, create_position_msg)?;

    // Give authorizations if not given already
    if !create_position {
        let client = AbstractClient::new(chain)?;
        give_authorizations(&client, carrot_app.addr_str()?)?;
    }
    Ok((pool_id, carrot_app))
}

pub fn give_authorizations_msgs<Chain: CwEnv>(
    client: &AbstractClient<Chain>,
    savings_app_addr: impl Into<String>,
) -> Result<Vec<Any>, anyhow::Error> {
    let dex_fee_account = client.fetch_account(AccountId::local(0))?;
    let dex_fee_addr = dex_fee_account.address()?.to_string();
    let chain = client.environment().clone();

    let authorization_urls = [
        MsgCreatePosition::TYPE_URL,
        MsgSwapExactAmountIn::TYPE_URL,
        MsgAddToPosition::TYPE_URL,
        MsgWithdrawPosition::TYPE_URL,
        MsgCollectIncentives::TYPE_URL,
        MsgCollectSpreadRewards::TYPE_URL,
    ]
    .map(ToOwned::to_owned);
    let savings_app_addr: String = savings_app_addr.into();
    let granter = chain.sender_addr().to_string();
    let grantee = savings_app_addr.clone();

    let dex_spend_limit = vec![
        cw_orch_osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: USDC_DENOM.to_owned(),
            amount: LOTS.to_string(),
        },
        cw_orch_osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: USDT_DENOM.to_owned(),
            amount: LOTS.to_string(),
        },
        cw_orch_osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: REWARD_DENOM.to_owned(),
            amount: LOTS.to_string(),
        }];
    let dex_fee_authorization = Any {
        value: MsgGrant {
            granter: chain.sender_addr().to_string(),
            grantee: grantee.clone(),
            grant: Some(Grant {
                authorization: Some(
                    SendAuthorization {
                        spend_limit: dex_spend_limit,
                        allow_list: vec![dex_fee_addr, savings_app_addr],
                    }
                    .to_any(),
                ),
                expiration: None,
            }),
        }
        .encode_to_vec(),
        type_url: MsgGrant::TYPE_URL.to_owned(),
    };

    let msgs: Vec<Any> = authorization_urls
        .into_iter()
        .map(|msg| Any {
            value: MsgGrant {
                granter: granter.clone(),
                grantee: grantee.clone(),
                grant: Some(Grant {
                    authorization: Some(GenericAuthorization { msg }.to_any()),
                    expiration: None,
                }),
            }
            .encode_to_vec(),
            type_url: MsgGrant::TYPE_URL.to_owned(),
        })
        .chain(iter::once(dex_fee_authorization))
        .collect();
    Ok(msgs)
}

pub fn give_authorizations<Chain: CwEnv + Stargate>(
    client: &AbstractClient<Chain>,
    savings_app_addr: impl Into<String>,
) -> Result<(), anyhow::Error> {
    let msgs = give_authorizations_msgs(client, savings_app_addr)?;
    client.environment().commit_any(msgs, None)?;
    Ok(())
}

pub mod incentives {
    use cw_orch_osmosis_test_tube::osmosis_test_tube::{
        fn_execute, fn_query,
        osmosis_std::types::osmosis::incentives::{
            MsgCreateGauge, MsgCreateGaugeResponse, QueryLockableDurationsRequest,
            QueryLockableDurationsResponse,
        },
        Module, Runner,
    };

    #[allow(unused)]
    pub struct Incentives<'a, R: Runner<'a>> {
        runner: &'a R,
    }

    impl<'a, R: Runner<'a>> Module<'a, R> for Incentives<'a, R> {
        fn new(runner: &'a R) -> Self {
            Self { runner }
        }
    }

    impl<'a, R> Incentives<'a, R>
    where
        R: Runner<'a>,
    {
        // macro for creating execute function
        fn_execute! {
            // (pub)? <fn_name>: <request_type> => <response_type>
            pub create_gauge: MsgCreateGauge => MsgCreateGaugeResponse
        }

        fn_query! {
            pub query_lockable_durations ["/osmosis.incentives.Query/LockableDurations"]: QueryLockableDurationsRequest => QueryLockableDurationsResponse
        }
    }
}
