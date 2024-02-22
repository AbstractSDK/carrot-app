use std::iter;

use abstract_app::abstract_interface::{Abstract, AbstractAccount};
use abstract_app::objects::salt::generate_instantiate_salt;
use abstract_app::{
    abstract_core::objects::{
        pool_id::PoolAddressBase, AccountId, AssetEntry, PoolMetadata, PoolType,
    },
    objects::module::ModuleVersion,
};
use abstract_client::{AbstractClient, Application, Environment, Namespace};
use abstract_interface::AccountFactoryQueryFns;
use carrot_app::contract::APP_ID;
use carrot_app::msg::{
    AppExecuteMsgFns, AppInstantiateMsg, AppQueryMsgFns, AssetsBalanceResponse,
    AvailableRewardsResponse, CompoundStatus, CompoundStatusResponse, CreatePositionMessage,
    PositionResponse,
};
use carrot_app::state::AutocompoundRewardsConfig;
use cosmwasm_std::{coin, coins, Decimal, Uint128, Uint64};
use cw_asset::AssetInfoUnchecked;
use cw_orch::osmosis_test_tube::osmosis_test_tube::{Account, Gamm};
use cw_orch::{
    anyhow,
    environment::WasmQuerier,
    osmosis_test_tube::osmosis_test_tube::{
        osmosis_std::types::{
            cosmos::{
                authz::v1beta1::{GenericAuthorization, Grant, MsgGrant, MsgGrantResponse},
                base::v1beta1,
            },
            osmosis::{
                concentratedliquidity::v1beta1::{
                    MsgCreatePosition, MsgWithdrawPosition, Pool, PoolsRequest,
                },
                gamm::v1beta1::MsgSwapExactAmountIn,
                tokenfactory::v1beta1::{MsgMint, MsgMintResponse},
            },
        },
        ConcentratedLiquidity, GovWithAppAccess, Module,
    },
    prelude::*,
};
use osmosis_std::types::cosmos::bank::v1beta1::SendAuthorization;
use osmosis_std::types::osmosis::{
    concentratedliquidity::v1beta1::{
        CreateConcentratedLiquidityPoolsProposal, MsgAddToPosition, MsgCollectIncentives,
        MsgCollectSpreadRewards, PoolRecord,
    },
    tokenfactory::v1beta1::{MsgCreateDenom, MsgCreateDenomResponse},
};
use prost::Message;
use prost_types::Any;

fn assert_is_around(result: Uint128, expected: impl Into<Uint128>) -> anyhow::Result<()> {
    let expected = expected.into().u128();
    let result = result.u128();

    if !(expected - 2..=expected + 2).contains(&result) {
        return Err(anyhow::anyhow!(
            "Results are not close enough, expected: {expected}, result: {result}"
        ));
    }
    Ok(())
}

fn factory_denom<Chain: CwEnv>(chain: &Chain, subdenom: &str) -> String {
    format!("factory/{}/{}", chain.sender(), subdenom)
}

fn create_denom<Chain: CwEnv + Stargate>(chain: Chain, subdenom: String) -> anyhow::Result<()> {
    chain.commit_any::<MsgCreateDenomResponse>(
        vec![Any {
            value: MsgCreateDenom {
                sender: chain.sender().to_string(),
                subdenom,
            }
            .encode_to_vec(),
            type_url: MsgCreateDenom::TYPE_URL.to_string(),
        }],
        None,
    )?;

    Ok(())
}

pub const LOTS: u128 = 100_000_000_000_000;

fn mint_lots_of_denom<Chain: CwEnv + Stargate>(
    chain: Chain,
    subdenom: String,
) -> anyhow::Result<()> {
    chain.commit_any::<MsgMintResponse>(
        vec![Any {
            value: MsgMint {
                sender: chain.sender().to_string(),
                amount: Some(coin(LOTS, factory_denom(&chain, &subdenom)).into()),
                mint_to_address: chain.sender().to_string(),
            }
            .encode_to_vec(),
            type_url: MsgMint::TYPE_URL.to_string(),
        }],
        None,
    )?;

    Ok(())
}

pub const USDC: &str = "USDC";
pub const USDT: &str = "USDT";
pub const REWARD_DENOM: &str = "reward";
pub const GAS_DENOM: &str = "uosmo";
pub const DEX_NAME: &str = "osmosis";

pub const TICK_SPACING: u64 = 100;
pub const SPREAD_FACTOR: u64 = 1;

pub const INITIAL_LOWER_TICK: i64 = -10000;
pub const INITIAL_UPPER_TICK: i64 = 1000;

// Deploys abstract and other contracts
pub fn deploy<Chain: CwEnv + Stargate>(
    chain: Chain,
    pool_id: u64,
    gas_pool_id: u64,
    create_position: Option<CreatePositionMessage>,
) -> anyhow::Result<Application<Chain, carrot_app::AppInterface<Chain>>> {
    let asset0 = factory_denom(&chain, USDC);
    let asset1 = factory_denom(&chain, USDT);
    // We register the pool inside the Abstract ANS
    let client = AbstractClient::builder(chain.clone())
        .dex(DEX_NAME)
        .assets(vec![
            (USDC.to_string(), AssetInfoUnchecked::Native(asset0.clone())),
            (USDT.to_string(), AssetInfoUnchecked::Native(asset1.clone())),
            (
                "rew".to_string(),
                AssetInfoUnchecked::Native(REWARD_DENOM.to_owned()),
            ),
        ])
        .pools(vec![
            (
                PoolAddressBase::Id(gas_pool_id),
                PoolMetadata {
                    dex: DEX_NAME.to_owned(),
                    pool_type: PoolType::ConcentratedLiquidity,
                    assets: vec![AssetEntry::new(USDC), AssetEntry::new("rew")],
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
        .build()?;
    // The dex adapter
    let dex_adapter = publisher
        .publish_adapter::<_, abstract_dex_adapter::interface::DexAdapter<Chain>>(
            abstract_dex_adapter::msg::DexInstantiateMsg {
                swap_fee: Decimal::percent(1),
                recipient_account: 0,
            },
        )?;
    // The savings app
    publisher.publish_app::<carrot_app::contract::interface::AppInterface<Chain>>()?;

    let app_code = client
        .version_control()
        .get_app_code(APP_ID, ModuleVersion::Latest)?;

    // If we create position on instantiate - give auth
    if create_position.is_some() {
        // TODO: We can't get account factory or module factory objects from the client.
        // get Account id of the upcoming sub-account
        let abs = Abstract::load_from(chain.clone())?;
        let account_factory_config = abs.account_factory.config()?;
        let next_local_account_id = AccountId::local(account_factory_config.local_account_sequence);

        // Get salt for the module
        let salt = generate_instantiate_salt(&next_local_account_id);

        let wasm_querier = chain.wasm_querier();

        let module_factory_addr = abs.module_factory.address()?;
        let savings_app_addr = wasm_querier
            .instantiate2_addr(app_code, module_factory_addr, salt)
            .unwrap();
        give_authorizations(&client, savings_app_addr)?;
    }

    // We deploy the carrot-app
    let carrot_app: Application<Chain, carrot_app::AppInterface<Chain>> =
        publisher
            .account()
            .install_app_with_dependencies::<carrot_app::contract::interface::AppInterface<Chain>>(
                &AppInstantiateMsg {
                    pool_id,
                    // 5 mins
                    autocompound_cooldown_seconds: Uint64::new(300),
                    autocompound_rewards_config: AutocompoundRewardsConfig {
                        gas_denom: REWARD_DENOM.to_owned(),
                        swap_denom: asset0,
                        reward: Uint128::new(1000),
                        min_gas_balance: Uint128::new(2000),
                        max_gas_balance: Uint128::new(10000),
                    },
                    create_position,
                },
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

    Ok(carrot_app)
}

fn create_position<Chain: CwEnv>(
    app: &Application<Chain, carrot_app::AppInterface<Chain>>,
    funds: Vec<Coin>,
    asset0: Coin,
    asset1: Coin,
) -> anyhow::Result<()> {
    app.execute(
        &carrot_app::msg::AppExecuteMsg::CreatePosition(CreatePositionMessage {
            lower_tick: INITIAL_LOWER_TICK,
            upper_tick: INITIAL_UPPER_TICK,
            funds,
            asset0,
            asset1,
        })
        .into(),
        None,
    )?;
    Ok(())
}

fn create_pool(chain: OsmosisTestTube) -> anyhow::Result<(u64, u64)> {
    // We create two tokenfactory denoms
    create_denom(chain.clone(), USDC.to_string())?;
    create_denom(chain.clone(), USDT.to_string())?;
    mint_lots_of_denom(chain.clone(), USDC.to_string())?;
    mint_lots_of_denom(chain.clone(), USDT.to_string())?;

    let asset0 = factory_denom(&chain, USDC);
    let asset1 = factory_denom(&chain, USDT);
    // Message for an actual chain (creating concentrated pool)
    // let create_pool_response = chain.commit_any::<MsgCreateConcentratedPoolResponse>(
    //     vec![Any {
    //         value: MsgCreateConcentratedPool {
    //             sender: chain.sender().to_string(),
    //             denom0: factory_denom(&chain, USDC),
    //             denom1: factory_denom(&chain, USDT),
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
                pool_records: vec![PoolRecord {
                    denom0: factory_denom(&chain, USDC),
                    denom1: factory_denom(&chain, USDT),
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

    let pool = Pool::decode(pools.pools[0].value.as_slice()).unwrap();
    let _response = cl
        .create_position(
            MsgCreatePosition {
                pool_id: pool.id,
                sender: chain.sender().to_string(),
                lower_tick: INITIAL_LOWER_TICK,
                upper_tick: INITIAL_UPPER_TICK,
                tokens_provided: vec![
                    v1beta1::Coin {
                        denom: asset0.clone(),
                        amount: "1_000_000".to_owned(),
                    },
                    v1beta1::Coin {
                        denom: asset1,
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

fn setup_test_tube(
    create_position: bool,
) -> anyhow::Result<(
    u64,
    Application<OsmosisTestTube, carrot_app::AppInterface<OsmosisTestTube>>,
)> {
    let _ = env_logger::builder().is_test(true).try_init();
    let chain = OsmosisTestTube::new(vec![
        coin(LOTS, GAS_DENOM),
        // All of it will get sent to the rewards pool
        // coin(2_000_000_000, REWARD_DENOM),
    ]);

    // We create a usdt-usdc pool
    let (pool_id, gas_pool_id) = create_pool(chain.clone())?;

    let create_position_msg = create_position.then(||
        // TODO: Requires instantiate2 to test it (we need to give authz authorization before instantiating)
        CreatePositionMessage {
        lower_tick: INITIAL_LOWER_TICK,
        upper_tick: INITIAL_UPPER_TICK,
        funds: coins(100_000, factory_denom(&chain, USDC)),
        asset0: coin(1_000_000, factory_denom(&chain, USDC)),
        asset1: coin(1_000_000, factory_denom(&chain, USDT)),
    });
    let carrot_app = deploy(chain.clone(), pool_id, gas_pool_id, create_position_msg)?;

    // Give authorizations if not given already
    if !create_position {
        let client = AbstractClient::new(chain)?;
        give_authorizations(&client, carrot_app.addr_str()?)?;
    }
    Ok((pool_id, carrot_app))
}

fn give_authorizations<Chain: CwEnv + Stargate>(
    client: &AbstractClient<Chain>,
    savings_app_addr: String,
) -> Result<(), anyhow::Error> {
    let dex_fee_account = client.account_from(AccountId::local(0))?;
    let dex_fee_addr = dex_fee_account.proxy()?.to_string();
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
    let granter = chain.sender().to_string();
    let grantee = savings_app_addr;

    let dex_spend_limit = vec![
        cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: factory_denom(&chain, USDC),
            amount: LOTS.to_string(),
        },
        cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: factory_denom(&chain, USDT),
            amount: LOTS.to_string(),
        }];
    let dex_fee_authorization = Any {
        value: MsgGrant {
            granter: chain.sender().to_string(),
            grantee: grantee.clone(),
            grant: Some(Grant {
                authorization: Some(
                    SendAuthorization {
                        spend_limit: dex_spend_limit,
                        allow_list: vec![dex_fee_addr],
                    }
                    .to_any(),
                ),
                expiration: None,
            }),
        }
        .encode_to_vec(),
        type_url: MsgGrant::TYPE_URL.to_owned(),
    };

    chain.commit_any::<MsgGrantResponse>(
        authorization_urls
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
            .collect(),
        None,
    )?;
    Ok(())
}

#[test]
fn deposit_lands() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.get_chain().clone();

    let deposit_amount = 5_000;
    let max_fee = Uint128::new(deposit_amount).mul_floor(Decimal::percent(1));
    // Create position
    create_position(
        &carrot_app,
        coins(deposit_amount, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > deposit_amount - max_fee.u128());

    // Do the deposit
    carrot_app.deposit(vec![coin(deposit_amount, factory_denom(&chain, USDC))])?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > (deposit_amount - max_fee.u128()) * 2);

    // Do the second deposit
    carrot_app.deposit(vec![coin(deposit_amount, factory_denom(&chain, USDC))])?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > (deposit_amount - max_fee.u128()) * 3);
    Ok(())
}

#[test]
fn withdraw_position() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.get_chain().clone();

    // Create position
    create_position(
        &carrot_app,
        coins(10_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let balance_usdc_before_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_before_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    // Withdraw half of liquidity
    let liquidity_amount: Uint128 = balance.liquidity.parse().unwrap();
    let half_of_liquidity = liquidity_amount / Uint128::new(2);
    carrot_app.withdraw(half_of_liquidity)?;

    let balance_usdc_after_half_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_after_half_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    assert!(balance_usdc_after_half_withdraw.amount > balance_usdc_before_withdraw.amount);
    assert!(balance_usdt_after_half_withdraw.amount > balance_usdt_before_withdraw.amount);

    // Withdraw rest of liquidity
    carrot_app.withdraw_all()?;
    let balance_usdc_after_full_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_after_full_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    assert!(balance_usdc_after_full_withdraw.amount > balance_usdc_after_half_withdraw.amount);
    assert!(balance_usdt_after_full_withdraw.amount > balance_usdt_after_half_withdraw.amount);
    Ok(())
}

#[test]
fn create_multiple_positions() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.get_chain().clone();

    // Create position
    create_position(
        &carrot_app,
        coins(10_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    let balances_first_position: AssetsBalanceResponse = carrot_app.balance()?;
    // Create position second time, user decided to close first one
    create_position(
        &carrot_app,
        coins(5_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    let balances_second_position: AssetsBalanceResponse = carrot_app.balance()?;

    // Should have more usd in total because it adds up
    let total_usd_first: Uint128 = balances_first_position
        .balances
        .into_iter()
        .map(|c| c.amount)
        .sum();
    let total_usd_second: Uint128 = balances_second_position
        .balances
        .into_iter()
        .map(|c| c.amount)
        .sum();
    assert!(total_usd_second > total_usd_first);

    // Should be at least (10_000 + 5_000) -2% with all fees
    assert!(total_usd_second > Uint128::new(15_000).mul_floor(Decimal::percent(98)));
    Ok(())
}

#[test]
fn deposit_both_assets() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.get_chain().clone();

    // Create position
    create_position(
        &carrot_app,
        coins(10_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    carrot_app.deposit(vec![
        coin(258, factory_denom(&chain, USDC)),
        coin(234, factory_denom(&chain, USDT)),
    ])?;

    Ok(())
}

#[test]
fn check_autocompound() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.get_chain().clone();

    // Create position
    create_position(
        &carrot_app,
        coins(100_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    // Do some swaps
    let dex: abstract_dex_adapter::interface::DexAdapter<_> = carrot_app.module()?;
    let abs = Abstract::load_from(chain.clone())?;
    let account_id = carrot_app.account().id()?;
    let account = AbstractAccount::new(&abs, account_id);
    chain.bank_send(
        account.proxy.addr_str()?,
        vec![
            coin(200_000, factory_denom(&chain, USDC)),
            coin(200_000, factory_denom(&chain, USDT)),
        ],
    )?;
    for _ in 0..10 {
        dex.ans_swap((USDC, 50_000), USDT, DEX_NAME.to_string(), &account)?;
        dex.ans_swap((USDT, 50_000), USDC, DEX_NAME.to_string(), &account)?;
    }

    // Check autocompound adds liquidity from the rewards and user balance remain unchanged

    // Check it has some rewards to autocompound first
    let rewards: AvailableRewardsResponse = carrot_app.available_rewards()?;
    assert!(!rewards.available_rewards.is_empty());

    // Save balances
    let balance_before_autocompound: AssetsBalanceResponse = carrot_app.balance()?;
    let balance_usdc_before_autocompound = chain
        .bank_querier()
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_before_autocompound = chain
        .bank_querier()
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    // Autocompound
    chain.wait_seconds(300)?;
    carrot_app.autocompound()?;

    // Save new balances
    let balance_after_autocompound: AssetsBalanceResponse = carrot_app.balance()?;
    let balance_usdc_after_autocompound = chain
        .bank_querier()
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_after_autocompound = chain
        .bank_querier()
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    // Liquidity added
    assert!(balance_after_autocompound.liquidity > balance_before_autocompound.liquidity);
    // Only rewards went in there
    assert_is_around(
        balance_usdc_after_autocompound.amount,
        balance_usdc_before_autocompound.amount,
    )
    .unwrap();
    assert_is_around(
        balance_usdt_after_autocompound.amount,
        balance_usdt_before_autocompound.amount,
    )
    .unwrap();

    // Check it used all of the rewards
    let rewards: AvailableRewardsResponse = carrot_app.available_rewards()?;
    assert!(rewards.available_rewards.is_empty());

    Ok(())
}

#[test]
fn stranger_autocompound() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let mut chain = carrot_app.get_chain().clone();
    let stranger = chain.init_account(coins(LOTS, GAS_DENOM))?;

    // Create position
    create_position(
        &carrot_app,
        coins(100_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    // Do some swaps
    let dex: abstract_dex_adapter::interface::DexAdapter<_> = carrot_app.module()?;
    let abs = Abstract::load_from(chain.clone())?;
    let account_id = carrot_app.account().id()?;
    let account = AbstractAccount::new(&abs, account_id);
    chain.bank_send(
        account.proxy.addr_str()?,
        vec![
            coin(200_000, factory_denom(&chain, USDC)),
            coin(200_000, factory_denom(&chain, USDT)),
        ],
    )?;
    for _ in 0..10 {
        dex.ans_swap((USDC, 50_000), USDT, DEX_NAME.to_string(), &account)?;
        dex.ans_swap((USDT, 50_000), USDC, DEX_NAME.to_string(), &account)?;
    }

    // Check autocompound adds liquidity from the rewards, user balance remain unchanged
    // and rewards gets passed to the "stranger"

    // Check it has some rewards to autocompound first
    let rewards: AvailableRewardsResponse = carrot_app.available_rewards()?;
    assert!(!rewards.available_rewards.is_empty());

    // Save balances
    let balance_before_autocompound: AssetsBalanceResponse = carrot_app.balance()?;

    // Autocompound by stranger
    chain.wait_seconds(300)?;
    // Check query is able to compute rewards, when swap is required
    let compound_status: CompoundStatusResponse = carrot_app.compound_status()?;
    assert_eq!(
        compound_status,
        CompoundStatusResponse {
            status: CompoundStatus::Ready {},
            reward: Coin::new(1000, REWARD_DENOM),
            rewards_available: true
        }
    );
    carrot_app.call_as(&stranger).autocompound()?;

    // Save new balances
    let balance_after_autocompound: AssetsBalanceResponse = carrot_app.balance()?;

    // Liquidity added
    assert!(balance_after_autocompound.liquidity > balance_before_autocompound.liquidity);

    // Check it used all of the rewards
    let rewards: AvailableRewardsResponse = carrot_app.available_rewards()?;
    assert!(rewards.available_rewards.is_empty());

    // Check stranger gets rewarded
    let stranger_reward_balance = chain.query_balance(stranger.address().as_str(), REWARD_DENOM)?;
    assert_eq!(stranger_reward_balance, Uint128::new(1000));
    Ok(())
}

#[test]
fn create_position_on_instantiation() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(true)?;

    let position: PositionResponse = carrot_app.position()?;
    assert!(position.position.is_some());
    Ok(())
}
