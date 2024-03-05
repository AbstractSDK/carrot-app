mod common;

use crate::common::{create_position, setup_test_tube, USDC, USDT};
use carrot_app::error::AppError;
use carrot_app::msg::{AppExecuteMsgFns, AppQueryMsgFns, AssetsBalanceResponse};
use cosmwasm_std::{coin, coins, Uint128};
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
