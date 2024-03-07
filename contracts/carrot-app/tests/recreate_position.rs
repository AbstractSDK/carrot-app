mod common;

use crate::common::{
    create_position, give_authorizations, setup_test_tube, INITIAL_LOWER_TICK, INITIAL_UPPER_TICK,
    USDC, USDT,
};
use abstract_app::objects::{AccountId, AssetEntry};
use abstract_client::{AbstractClient, Environment};
use carrot_app::error::AppError;
use carrot_app::msg::{
    AppExecuteMsgFns, AppInstantiateMsg, AppQueryMsgFns, AssetsBalanceResponse,
    CreatePositionMessage, PositionResponse,
};
use carrot_app::state::AutocompoundRewardsConfig;
use common::REWARD_ASSET;
use cosmwasm_std::{coin, coins, Uint128, Uint64};
use cw_orch::{
    anyhow,
    osmosis_test_tube::osmosis_test_tube::{
        osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgWithdrawPosition,
        ConcentratedLiquidity, Module,
    },
    prelude::*,
};
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::PositionByIdRequest;

#[test]
fn create_multiple_positions() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    // Create position
    create_position(
        &carrot_app,
        coins(10_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    // Create position second time, it should fail
    let position_err = create_position(
        &carrot_app,
        coins(5_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )
    .unwrap_err();

    assert!(position_err
        .to_string()
        .contains(&AppError::PositionExists {}.to_string()));
    Ok(())
}

#[test]
fn create_multiple_positions_after_withdraw() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    // Create position
    create_position(
        &carrot_app,
        coins(10_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    // Withdraw half of liquidity
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let liquidity_amount: Uint128 = balance.liquidity.parse().unwrap();
    let half_of_liquidity = liquidity_amount / Uint128::new(2);
    carrot_app.withdraw(half_of_liquidity)?;

    // Create position second time, it should fail
    let position_err = create_position(
        &carrot_app,
        coins(5_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )
    .unwrap_err();

    assert!(position_err
        .to_string()
        .contains(&AppError::PositionExists {}.to_string()));

    // Withdraw whole liquidity
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let liquidity_amount: Uint128 = balance.liquidity.parse().unwrap();
    carrot_app.withdraw(liquidity_amount)?;

    // Create position second time, it should fail
    create_position(
        &carrot_app,
        coins(5_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    Ok(())
}

#[test]
fn create_multiple_positions_after_withdraw_all() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    // Create position
    create_position(
        &carrot_app,
        coins(10_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    // Withdraw whole liquidity
    carrot_app.withdraw_all()?;

    // Create position second time, it should succeed
    create_position(
        &carrot_app,
        coins(5_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;
    Ok(())
}

#[test]
fn create_position_after_user_withdraw_liquidity_manually() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(true)?;
    let chain = carrot_app.get_chain().clone();

    let position = carrot_app.position()?;

    let test_tube = chain.app.borrow();
    let cl = ConcentratedLiquidity::new(&*test_tube);
    let position_breakdown = cl
        .query_position_by_id(&PositionByIdRequest {
            position_id: position.position.unwrap().position_id,
        })?
        .position
        .unwrap();
    let position = position_breakdown.position.unwrap();

    cl.withdraw_position(
        MsgWithdrawPosition {
            position_id: position.position_id,
            sender: chain.sender().to_string(),
            liquidity_amount: position.liquidity,
        },
        &chain.sender,
    )?;

    // Create position, ignoring it was manually withdrawn
    create_position(
        &carrot_app,
        coins(10_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    let position = carrot_app.position()?;
    assert!(position.position.is_some());
    Ok(())
}

#[test]
fn install_on_sub_account() -> anyhow::Result<()> {
    let (pool_id, app) = setup_test_tube(false)?;
    let owner_account = app.account();
    let chain = owner_account.environment();
    let client = AbstractClient::new(chain)?;
    let next_id = client.next_local_account_id()?;

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
        create_position: None,
    };

    let account = client
        .account_builder()
        .sub_account(owner_account)
        .account_id(next_id)
        .name("carrot-sub-acc")
        .install_app_with_dependencies::<carrot_app::contract::interface::AppInterface<OsmosisTestTube>>(
            &init_msg,
            Empty {},
        )?
        .build()?;

    let carrot_app = account.application::<carrot_app::AppInterface<_>>()?;

    give_authorizations(&client, carrot_app.addr_str()?)?;
    create_position(
        &carrot_app,
        coins(10_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    let position: PositionResponse = carrot_app.position()?;
    assert!(position.position.is_some());
    Ok(())
}

#[test]
fn install_on_sub_account_create_position_on_install() -> anyhow::Result<()> {
    let (pool_id, app) = setup_test_tube(false)?;
    let owner_account = app.account();
    let chain = owner_account.environment();
    let client = AbstractClient::new(chain)?;
    let next_id = client.next_local_account_id()?;
    let carrot_app_address = client
        .module_instantiate2_address::<carrot_app::AppInterface<OsmosisTestTube>>(
            &AccountId::local(next_id),
        )?;

    give_authorizations(&client, carrot_app_address)?;
    let init_msg = AppInstantiateMsg {
        pool_id,
        // 5 mins
        autocompound_cooldown_seconds: Uint64::new(300),
        autocompound_rewards_config: AutocompoundRewardsConfig {
            gas_asset: AssetEntry::new(REWARD_ASSET),
            swap_asset: AssetEntry::new(USDC),
            reward: Uint128::new(500_000),
            min_gas_balance: Uint128::new(1_000_000),
            max_gas_balance: Uint128::new(3_000_000),
        },
        create_position: Some(CreatePositionMessage {
            lower_tick: INITIAL_LOWER_TICK,
            upper_tick: INITIAL_UPPER_TICK,
            funds: coins(100_000, USDC),
            asset0: coin(1_000_672_899, USDT),
            asset1: coin(10_000_000_000, USDC),
        }),
    };

    let account = client
        .account_builder()
        .sub_account(owner_account)
        .account_id(next_id)
        .name("carrot-sub-acc")
        .install_app_with_dependencies::<carrot_app::contract::interface::AppInterface<OsmosisTestTube>>(
            &init_msg,
            Empty {},
        )?
        .build()?;

    let carrot_app = account.application::<carrot_app::AppInterface<_>>()?;

    let position: PositionResponse = carrot_app.position()?;
    assert!(position.position.is_some());
    Ok(())
}
