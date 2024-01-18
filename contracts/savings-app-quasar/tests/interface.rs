use abstract_client::Application;
use abstract_client::Environment;
use abstract_client::Namespace;
use abstract_core::objects::AssetEntry;
use abstract_core::objects::PoolMetadata;
use abstract_core::objects::PoolType;
use abstract_core::objects::pool_id::PoolAddressBase;
use abstract_dex_adapter::msg::OfferAsset;
use abstract_interface::Abstract;
use cl_vault::state::VaultConfig;
use cosmwasm_std::Decimal;
use cosmwasm_std::coin;
use cw_asset::AssetInfoUnchecked;
use cw_orch::anyhow;
use cw_orch::build::BuildPostfix;
use cw_orch::osmosis_test_tube::osmosis_test_tube::ConcentratedLiquidity;
use cw_orch::osmosis_test_tube::osmosis_test_tube::GovWithAppAccess;
use cw_orch::osmosis_test_tube::osmosis_test_tube::Module;
use cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::osmosis::concentratedliquidity::poolmodel::concentrated::v1beta1::{
    MsgCreateConcentratedPool, MsgCreateConcentratedPoolResponse};
use cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgCreatePosition;
use cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::osmosis::concentratedliquidity::v1beta1::Pool;
use cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::osmosis::concentratedliquidity::v1beta1::PoolsRequest;
use cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::osmosis::tokenfactory::v1beta1::MsgMint;
use cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::osmosis::tokenfactory::v1beta1::MsgMintResponse;
use cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1;
use cw_orch::prelude::*;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::CreateConcentratedLiquidityPoolsProposal;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::PoolRecord;
use osmosis_std::types::osmosis::tokenfactory::v1beta1::MsgCreateDenom;
use osmosis_std::types::osmosis::tokenfactory::v1beta1::MsgCreateDenomResponse;
use prost::Message;
use prost_types::Any;
use abstract_client::AbstractClient;
use cosmwasm_std::coins;
use app::msg::AppInstantiateMsg;
use app::msg::AppExecuteMsgFns;

#[cw_orch::interface(
    app::cl_vault::msg::InstantiateMsg,
    app::cl_vault::msg::QueryMsg,
    app::cl_vault::msg::ExecuteMsg,
    app::cl_vault::msg::MigrateMsg
)]
pub struct QuasarPool;

impl<Chain: CwEnv> Uploadable for QuasarPool<Chain> {
    fn wasm(&self) -> WasmPath {
        artifacts_dir_from_workspace!()
            .find_wasm_path("cl_vault")
            .unwrap()
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
pub const SPREAD_FACTOR: &str = "10";

pub const INITIAL_LOWER_TICK: i64 = -100;
pub const INITIAL_UPPER_TICK: i64 = 100;

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

    // We deploy quasar on top of this pool
    let quasar_pool = QuasarPool::new("quasar:cl-vault", chain.clone());
    quasar_pool.upload()?;
    quasar_pool.instantiate(
        &app::cl_vault::msg::InstantiateMsg {
            thesis: "You can become rich dear person".to_string(),
            name: VAULT_NAME.to_string(),
            admin: chain.sender().to_string(),
            range_admin: chain.sender().to_string(),
            pool_id,
            config: VaultConfig {
                performance_fee: Decimal::percent(20),
                treasury: chain.sender(),
                swap_max_slippage: Decimal::percent(1),
            },
            vault_token_subdenom: VAULT_SUBDENOM.to_string(),
            initial_lower_tick: INITIAL_LOWER_TICK,
            initial_upper_tick: INITIAL_UPPER_TICK,
        },
        None,
        // Need to deposit few asset to create initial position, pretending those tokens are burned
        Some(&[coin(2, asset0.clone()), coin(2, asset1)]),
    )?;

    // We deploy the savings-app on top of the quasar contract
    let savings_app: Application<Chain, app::AppInterface<Chain>> =
        publisher
            .account()
            .install_app_with_dependencies::<app::contract::interface::AppInterface<Chain>>(
                &AppInstantiateMsg {
                    deposit_denom: asset0,
                    quasar_pool: quasar_pool.address()?.to_string(),
                    exchanges: vec![DEX_NAME.to_string()],
                },
                Empty {},
                &[],
            )?;

    Ok(savings_app)
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
                    tick_spacing: 1,
                    spread_factor: "0".to_string(),
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
    cl.create_position(
        MsgCreatePosition {
            pool_id: pool.id,
            sender: chain.sender().to_string(),
            lower_tick: INITIAL_LOWER_TICK,
            upper_tick: INITIAL_UPPER_TICK,
            tokens_provided: vec![
                v1beta1::Coin {
                    denom: asset0,
                    amount: "100_000_000".to_owned(),
                },
                v1beta1::Coin {
                    denom: asset1,
                    amount: "100_000_000".to_owned(),
                },
            ],
            token_min_amount0: "0".to_string(),
            token_min_amount1: "0".to_string(),
        },
        &chain.sender,
    )?;
    Ok(pool.id)
}

fn setup_test_tube() -> anyhow::Result<(
    u64,
    Application<OsmosisTestTube, app::AppInterface<OsmosisTestTube>>,
)> {
    dotenv::dotenv()?;
    env_logger::init();
    let chain = OsmosisTestTube::new(coins(LOTS, "uosmo"));
    // We create a usdt-usdc pool
    let pool_id = create_pool(chain.clone())?;

    let savings_app = deploy(chain, pool_id)?;
    Ok((pool_id, savings_app))
}

#[test]
fn deposit_lands() -> anyhow::Result<()> {
    let (_, savings_app) = setup_test_tube()?;
    let chain = savings_app.get_chain().clone();
    let chain_name: String = BuildPostfix::<OsmosisTestTube>::ChainName(&chain).into();
    println!("chain_name: {chain_name}");
    // Checking why simulate_swap fails:
    // let abstract = Abstract::load_from(chain)?;
    use abstract_dex_adapter::msg::DexQueryMsgFns as _;
    let dex_adapter: abstract_dex_adapter::interface::DexAdapter<_> = savings_app.module()?;
    let resp = dex_adapter.simulate_swap(
        AssetEntry::new(USDT),
        OfferAsset::new(USDC, 2_u128),
        Some(DEX_NAME.to_owned()),
    );
    println!("{resp:?}");

    // savings_app.deposit(&[coin(100, factory_denom(&chain, USDC))])?;
    Ok(())
}
