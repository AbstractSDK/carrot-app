use abstract_app::abstract_core::{
    adapter::{AdapterBaseMsg, BaseExecuteMsg},
    objects::{pool_id::PoolAddressBase, AccountId, AssetEntry, PoolMetadata, PoolType},
};
use abstract_app::abstract_interface::{Abstract, AbstractAccount};
use abstract_client::{AbstractClient, Application, Namespace};
use abstract_dex_adapter::msg::ExecuteMsg;
use app::msg::{
    AppExecuteMsgFns, AppInstantiateMsg, AppQueryMsgFns, AssetsBalanceResponse,
    AvailableRewardsResponse,
};
use cosmwasm_std::{coin, coins, Decimal, Uint128};
use cw_asset::AssetInfoUnchecked;
use cw_orch::{
    anyhow,
    environment::BankQuerier,
    osmosis_test_tube::osmosis_test_tube::{
        osmosis_std::types::{
            cosmos::{
                authz::v1beta1::{GenericAuthorization, Grant, MsgGrant, MsgGrantResponse},
                bank::v1beta1::SendAuthorization,
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
        ConcentratedLiquidity, ExecuteResponse, GovWithAppAccess, Module, Runner,
    },
    prelude::*,
};
use osmosis_std::types::osmosis::{
    concentratedliquidity::v1beta1::{
        CreateConcentratedLiquidityPoolsProposal, MsgAddToPosition, MsgCollectIncentives,
        MsgCollectSpreadRewards, PoolRecord,
    },
    tokenfactory::v1beta1::{MsgCreateDenom, MsgCreateDenomResponse},
};
use prost::Message;
use prost_types::Any;

fn assert_is_around(result: Uint128, expected: impl Into<Uint128>) {
    let expected = expected.into().u128();
    let result = result.u128();

    if expected < result - 1 || expected > result + 1 {
        panic!("Results are not close enough")
    }
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
pub const DEX_NAME: &str = "osmosis";
pub const VAULT_NAME: &str = "quasar_vault";
pub const VAULT_SUBDENOM: &str = "vault-token";

pub const TICK_SPACING: u64 = 100;
pub const SPREAD_FACTOR: u64 = 1;

pub const INITIAL_LOWER_TICK: i64 = -10000;
pub const INITIAL_UPPER_TICK: i64 = 1000;

// Deploys abstract and other contracts
pub fn deploy<Chain: CwEnv + Stargate>(
    chain: Chain,
    pool_id: u64,
) -> anyhow::Result<Application<Chain, app::AppInterface<Chain>>> {
    let asset0 = factory_denom(&chain, USDC);
    let asset1 = factory_denom(&chain, USDT);
    // We register the pool inside the Abstract ANS
    let client = AbstractClient::builder(chain.clone())
        .dex(DEX_NAME)
        .assets(vec![
            (USDC.to_string(), AssetInfoUnchecked::Native(asset0.clone())),
            (USDT.to_string(), AssetInfoUnchecked::Native(asset1.clone())),
        ])
        .pool(
            PoolAddressBase::Id(pool_id),
            PoolMetadata {
                dex: DEX_NAME.to_owned(),
                pool_type: PoolType::ConcentratedLiquidity,
                assets: vec![AssetEntry::new(USDC), AssetEntry::new(USDT)],
            },
        )
        .build()?;

    // We deploy the app
    let publisher = client
        .publisher_builder(Namespace::new("abstract")?)
        .build()?;
    // The dex adapter
    publisher.publish_adapter::<_, abstract_dex_adapter::interface::DexAdapter<Chain>>(
        abstract_dex_adapter::msg::DexInstantiateMsg {
            swap_fee: Decimal::percent(1),
            recipient_account: 0,
        },
    )?;
    // The savings app
    publisher.publish_app::<app::contract::interface::AppInterface<Chain>>()?;

    // We deploy the savings-app
    let savings_app: Application<Chain, app::AppInterface<Chain>> =
        publisher
            .account()
            .install_app_with_dependencies::<app::contract::interface::AppInterface<Chain>>(
                &AppInstantiateMsg {
                    deposit_denom: asset0,
                    exchanges: vec![DEX_NAME.to_string()],
                    pool_id,
                },
                Empty {},
                &[],
            )?;

    // We update authorized addresses on the adapter for the app
    savings_app
        .account()
        .execute_on_module::<abstract_dex_adapter::interface::DexAdapter<Chain>>(
            &ExecuteMsg::Base(BaseExecuteMsg {
                proxy_address: Some(savings_app.account().proxy()?.to_string()),
                msg: AdapterBaseMsg::UpdateAuthorizedAddresses {
                    to_add: vec![savings_app.addr_str()?],
                    to_remove: vec![],
                },
            }),
            &[],
        )?;

    Ok(savings_app)
}

fn create_position<Chain: CwEnv>(
    app: &Application<Chain, app::AppInterface<Chain>>,
    funds: Vec<Coin>,
    asset0: Coin,
    asset1: Coin,
) -> anyhow::Result<()> {
    app.account()
        .execute_on_module::<app::AppInterface<Chain>>(
            &app::msg::AppExecuteMsg::CreatePosition {
                lower_tick: INITIAL_LOWER_TICK,
                upper_tick: INITIAL_UPPER_TICK,
                funds,
                asset0,
                asset1,
            }
            .into(),
            &[],
        )?;
    Ok(())
}

fn create_pool(chain: OsmosisTestTube) -> anyhow::Result<u64> {
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
                        denom: asset0,
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

    Ok(pool.id)
}

fn setup_test_tube() -> anyhow::Result<(
    u64,
    Application<OsmosisTestTube, app::AppInterface<OsmosisTestTube>>,
)> {
    dotenv::dotenv()?;
    let _ = env_logger::builder().is_test(true).try_init();
    let chain = OsmosisTestTube::new(coins(LOTS, "uosmo"));
    // We create a usdt-usdc pool
    let pool_id = create_pool(chain.clone())?;

    let savings_app = deploy(chain, pool_id)?;

    // Give authorizations
    give_authorizations(&savings_app)?;
    Ok((pool_id, savings_app))
}

fn give_authorizations(
    savings_app: &Application<OsmosisTestTube, app::AppInterface<OsmosisTestTube>>,
) -> Result<(), anyhow::Error> {
    let chain = savings_app.get_chain();
    let abs = Abstract::load_from(chain.clone())?;
    let dex_fee_account = AbstractAccount::new(&abs, AccountId::local(0));
    let dex_fee_addr = dex_fee_account.proxy.addr_str()?;

    let app = chain.app.borrow();
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
    let grantee = savings_app.addr_str().unwrap();
    for msg in authorization_urls {
        let _: ExecuteResponse<MsgGrantResponse> = app.execute(
            MsgGrant {
                granter: granter.clone(),
                grantee: grantee.clone(),
                grant: Some(Grant {
                    authorization: Some(GenericAuthorization { msg }.to_any()),
                    expiration: None,
                }),
            },
            MsgGrant::TYPE_URL,
            chain.sender.as_ref(),
        )?;
    }
    // Dex fees
    let _: ExecuteResponse<MsgGrantResponse> = app.execute(
        MsgGrant {
            granter: chain.sender().to_string(),
            grantee: savings_app.addr_str().unwrap(),
            grant: Some(Grant {
                authorization: Some(
                    SendAuthorization {
                        spend_limit: vec![
                            cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
                                denom: factory_denom(chain, USDC),
                                amount: LOTS.to_string(),
                            },
                            cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
                                denom: factory_denom(chain, USDT),
                                amount: LOTS.to_string(),
                            },
                        ],
                        allow_list: vec![dex_fee_addr],
                    }
                    .to_any(),
                ),
                expiration: None,
            }),
        },
        MsgGrant::TYPE_URL,
        chain.sender.as_ref(),
    )?;

    Ok(())
}

#[test]
fn deposit_lands() -> anyhow::Result<()> {
    let (_, savings_app) = setup_test_tube()?;

    let chain = savings_app.get_chain().clone();

    let deposit_amount = 5_000;
    let max_fee = Uint128::new(deposit_amount).mul_floor(Decimal::percent(1));
    // Create position
    create_position(
        &savings_app,
        coins(deposit_amount, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = savings_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > deposit_amount - max_fee.u128());

    // Do the deposit
    savings_app.deposit(vec![coin(deposit_amount, factory_denom(&chain, USDC))])?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = savings_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > (deposit_amount - max_fee.u128()) * 2);

    // Do the second deposit
    savings_app.deposit(vec![coin(deposit_amount, factory_denom(&chain, USDC))])?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = savings_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > (deposit_amount - max_fee.u128()) * 3);
    Ok(())
}

#[test]
fn withdraw_position() -> anyhow::Result<()> {
    let (_, savings_app) = setup_test_tube()?;

    let chain = savings_app.get_chain().clone();

    // Create position
    create_position(
        &savings_app,
        coins(10_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    let balance: AssetsBalanceResponse = savings_app.balance()?;
    let balance_usdc_before_withdraw = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_before_withdraw = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    // Withdraw half of liquidity
    let liquidity_amount: Uint128 = balance.liquidity.parse().unwrap();
    let half_of_liquidity = liquidity_amount / Uint128::new(2);
    savings_app.withdraw(half_of_liquidity)?;

    let balance_usdc_after_half_withdraw = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_after_half_withdraw = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    assert!(balance_usdc_after_half_withdraw.amount > balance_usdc_before_withdraw.amount);
    assert!(balance_usdt_after_half_withdraw.amount > balance_usdt_before_withdraw.amount);

    // Withdraw rest of liquidity
    savings_app.withdraw_all()?;
    let balance_usdc_after_full_withdraw = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_after_full_withdraw = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    assert!(balance_usdc_after_full_withdraw.amount > balance_usdc_after_half_withdraw.amount);
    assert!(balance_usdt_after_full_withdraw.amount > balance_usdt_after_half_withdraw.amount);
    Ok(())
}

#[test]
fn create_multiple_positions() -> anyhow::Result<()> {
    let (_, savings_app) = setup_test_tube()?;

    let chain = savings_app.get_chain().clone();

    // Create position
    create_position(
        &savings_app,
        coins(10_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    let balance_usdc_first_position = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_first_position = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();
    // Create position second time, user decided to close first one
    create_position(
        &savings_app,
        coins(5_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    let balance_usdc_second_position = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_second_position = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    // Should have more usd in total because we did withdraw before creating new position
    assert!(
        balance_usdc_second_position.amount + balance_usdt_second_position.amount
            > balance_usdc_first_position.amount + balance_usdt_first_position.amount
    );
    Ok(())
}

#[test]
fn deposit_both_assets() -> anyhow::Result<()> {
    let (_, savings_app) = setup_test_tube()?;

    let chain = savings_app.get_chain().clone();

    // Create position
    create_position(
        &savings_app,
        coins(10_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    savings_app.deposit(vec![
        coin(258, factory_denom(&chain, USDC)),
        coin(234, factory_denom(&chain, USDT)),
    ])?;

    Ok(())
}

#[test]
fn check_autocompound() -> anyhow::Result<()> {
    let (_, savings_app) = setup_test_tube()?;

    let chain = savings_app.get_chain().clone();

    // Create position
    create_position(
        &savings_app,
        coins(100_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    // Do some swaps
    let dex: abstract_dex_adapter::interface::DexAdapter<_> = savings_app.module()?;
    let abs = Abstract::load_from(chain.clone())?;
    let account_id = savings_app.account().id()?;
    let account = AbstractAccount::new(&abs, account_id);
    chain.bank_send(
        account.proxy.addr_str()?,
        vec![
            coin(200_000, factory_denom(&chain, USDC)),
            coin(200_000, factory_denom(&chain, USDT)),
        ],
    )?;
    for _ in 0..10 {
        dex.swap((USDC, 50_000), USDT, DEX_NAME.to_string(), &account)?;
        dex.swap((USDT, 50_000), USDC, DEX_NAME.to_string(), &account)?;
    }

    // Check autocompound adds liquidity from the rewards and user balance remain unchanged

    // Check it has some rewards to autocompound first
    let rewards: AvailableRewardsResponse = savings_app.available_rewards()?;
    assert!(!rewards.available_rewards.is_empty());

    // Save balances
    let balance_before_autocompound: AssetsBalanceResponse = savings_app.balance()?;
    let balance_usdc_before_autocompound = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_before_autocompound = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    // Autocompound
    savings_app.autocompound()?;

    // Save new balances
    let balance_after_autocompound: AssetsBalanceResponse = savings_app.balance()?;
    let balance_usdc_after_autocompound = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDC)))?
        .pop()
        .unwrap();
    let balance_usdt_after_autocompound = chain
        .balance(chain.sender(), Some(factory_denom(&chain, USDT)))?
        .pop()
        .unwrap();

    // Liquidity added
    assert!(balance_after_autocompound.liquidity > balance_before_autocompound.liquidity);
    // Only rewards went in there
    assert_is_around(
        balance_usdc_after_autocompound.amount,
        balance_usdc_before_autocompound.amount,
    );
    assert_is_around(
        balance_usdt_after_autocompound.amount,
        balance_usdt_before_autocompound.amount,
    );

    // Check it used all of the rewards
    let rewards: AvailableRewardsResponse = savings_app.available_rewards()?;
    assert!(rewards.available_rewards.is_empty());

    Ok(())
}
