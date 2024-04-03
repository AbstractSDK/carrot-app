mod common;

use crate::common::{create_position, setup_test_tube, USDC, USDT};
use abstract_interface::{Abstract, AbstractAccount};
use carrot_app::msg::{AppExecuteMsgFns, AppQueryMsgFns, AssetsBalanceResponse, PositionResponse};
use common::DEX_NAME;
use cosmwasm_std::{coin, coins, Decimal, Uint128};
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
fn deposit_lands() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let deposit_amount = 5_000;
    let max_fee = Uint128::new(deposit_amount).mul_floor(Decimal::percent(3));
    // Create position
    create_position(
        &carrot_app,
        coins(deposit_amount, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > deposit_amount - max_fee.u128());

    // Do the deposit
    carrot_app.deposit(
        vec![coin(deposit_amount, USDT.to_owned())],
        None,
        None,
        None,
    )?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > (deposit_amount - max_fee.u128()) * 2);

    // Do the second deposit
    carrot_app.deposit(
        vec![coin(deposit_amount, USDT.to_owned())],
        None,
        None,
        None,
    )?;
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
        coins(10_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let balance_usdc_before_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(USDT.to_owned()))?
        .pop()
        .unwrap();
    let balance_usdt_before_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(USDC.to_owned()))?
        .pop()
        .unwrap();

    // Withdraw half of liquidity
    let liquidity_amount: Uint128 = balance.liquidity.parse().unwrap();
    let half_of_liquidity = liquidity_amount / Uint128::new(2);
    carrot_app.withdraw(half_of_liquidity)?;

    let balance_usdc_after_half_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(USDT.to_owned()))?
        .pop()
        .unwrap();
    let balance_usdt_after_half_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(USDC.to_owned()))?
        .pop()
        .unwrap();

    assert!(balance_usdc_after_half_withdraw.amount > balance_usdc_before_withdraw.amount);
    assert!(balance_usdt_after_half_withdraw.amount > balance_usdt_before_withdraw.amount);

    // Withdraw rest of liquidity
    carrot_app.withdraw_all()?;
    let balance_usdc_after_full_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(USDT.to_owned()))?
        .pop()
        .unwrap();
    let balance_usdt_after_full_withdraw = chain
        .bank_querier()
        .balance(chain.sender(), Some(USDC.to_owned()))?
        .pop()
        .unwrap();

    assert!(balance_usdc_after_full_withdraw.amount > balance_usdc_after_half_withdraw.amount);
    assert!(balance_usdt_after_full_withdraw.amount > balance_usdt_after_half_withdraw.amount);
    Ok(())
}

#[test]
fn deposit_both_assets() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    // Create position
    create_position(
        &carrot_app,
        coins(10_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    carrot_app.deposit(
        vec![coin(258, USDT.to_owned()), coin(234, USDC.to_owned())],
        None,
        None,
        None,
    )?;

    Ok(())
}

#[test]
fn create_position_on_instantiation() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(true)?;

    let position: PositionResponse = carrot_app.position()?;
    assert!(position.position.is_some());
    Ok(())
}

#[test]
fn withdraw_after_user_withdraw_liquidity_manually() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(true)?;
    let chain = carrot_app.get_chain().clone();

    let position: PositionResponse = carrot_app.position()?;
    let position_id = position.position.unwrap().position_id;

    let test_tube = chain.app.borrow();
    let cl = ConcentratedLiquidity::new(&*test_tube);
    let position_breakdown = cl
        .query_position_by_id(&PositionByIdRequest { position_id })?
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

    // Ensure it errors
    carrot_app.withdraw_all().unwrap_err();

    // Ensure position deleted
    let position_not_found = cl
        .query_position_by_id(&PositionByIdRequest { position_id })
        .unwrap_err();
    assert!(position_not_found
        .to_string()
        .contains("position not found"));
    Ok(())
}

#[test]
fn deposit_slippage() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let deposit_amount = 5_000;
    let max_fee = Uint128::new(deposit_amount).mul_floor(Decimal::percent(3));
    // Create position
    create_position(
        &carrot_app,
        coins(deposit_amount, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    // Do the deposit of asset0 with incorrect belief_price1
    let e = carrot_app
        .deposit(
            vec![coin(deposit_amount, USDT.to_owned())],
            None,
            Some(Decimal::zero()),
            None,
        )
        .unwrap_err();
    assert!(e.to_string().contains("exceeds max spread limit"));

    // Do the deposit of asset1 with incorrect belief_price0
    let e = carrot_app
        .deposit(
            vec![coin(deposit_amount, USDC.to_owned())],
            Some(Decimal::zero()),
            None,
            None,
        )
        .unwrap_err();
    assert!(e.to_string().contains("exceeds max spread limit"));

    // Do the deposits of asset0 with correct belief_price
    carrot_app.deposit(
        vec![coin(deposit_amount, USDT.to_owned())],
        None,
        Some(Decimal::one()),
        Some(Decimal::percent(10)),
    )?;
    // Do the deposits of asset1 with correct belief_price
    carrot_app.deposit(
        vec![coin(deposit_amount, USDT.to_owned())],
        Some(Decimal::one()),
        None,
        Some(Decimal::percent(10)),
    )?;

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
fn withdraw_position_autoclaims() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.get_chain().clone();

    // Create position
    create_position(
        &carrot_app,
        coins(10_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    // Do some swaps
    let dex: abstract_dex_adapter::interface::DexAdapter<_> = carrot_app.module()?;
    let abs = Abstract::load_from(chain.clone())?;
    let account_id = carrot_app.account().id()?;
    let account = AbstractAccount::new(&abs, account_id);
    chain.bank_send(
        account.proxy.addr_str()?,
        vec![
            coin(200_000, USDC.to_owned()),
            coin(200_000, USDT.to_owned()),
        ],
    )?;
    for _ in 0..10 {
        dex.ans_swap((USDC, 50_000), USDT, DEX_NAME.to_string(), &account)?;
        dex.ans_swap((USDT, 50_000), USDC, DEX_NAME.to_string(), &account)?;
    }

    // Withdraw half of liquidity
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let liquidity_amount: Uint128 = balance.liquidity.parse().unwrap();
    let half_of_liquidity = liquidity_amount / Uint128::new(2);
    carrot_app.withdraw(half_of_liquidity)?;

    Ok(())
}
