mod common;

use crate::common::{setup_test_tube, USDC, USDT};
use carrot_app::{
    msg::{AppExecuteMsgFns, AppQueryMsgFns, AssetsBalanceResponse},
    yield_sources::osmosis_cl_pool::OsmosisPosition,
};
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

    // We should add funds to the account proxy
    let deposit_coins = coins(deposit_amount, USDT.to_owned());
    let mut chain = carrot_app.get_chain().clone();
    chain.add_balance(
        carrot_app.account().proxy()?.to_string(),
        deposit_coins.clone(),
    )?;

    // Do the deposit
    carrot_app.deposit(deposit_coins.clone())?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > (deposit_amount - max_fee.u128()) * 2);

    // Do the second deposit
    carrot_app.deposit(vec![coin(deposit_amount, USDT.to_owned())])?;
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

    carrot_app.deposit(coins(10_000, USDT.to_owned()))?;

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

    // Withdraw some of the value
    let liquidity_amount: Uint128 = balance.balances[0].amount;
    let half_of_liquidity = liquidity_amount / Uint128::new(2);
    carrot_app.withdraw(Some(half_of_liquidity))?;

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
    carrot_app.withdraw(None)?;
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
fn deposit_multiple_assets() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    carrot_app.deposit(vec![coin(258, USDT.to_owned()), coin(234, USDC.to_owned())])?;

    Ok(())
}

// #[test]
// fn create_position_on_instantiation() -> anyhow::Result<()> {
//     let (_, carrot_app) = setup_test_tube(true)?;
//     carrot_app.deposit(vec![coin(258, USDT.to_owned()), coin(234, USDC.to_owned())])?;

//     let position: OsmosisPositionResponse = carrot_app.position()?;
//     assert!(position.position.is_some());
//     Ok(())
// }

// #[test]
// fn withdraw_after_user_withdraw_liquidity_manually() -> anyhow::Result<()> {
//     let (_, carrot_app) = setup_test_tube(true)?;
//     let chain = carrot_app.get_chain().clone();

//     let position: PositionResponse = carrot_app.position()?;
//     let position_id = position.position.unwrap().position_id;

//     let test_tube = chain.app.borrow();
//     let cl = ConcentratedLiquidity::new(&*test_tube);
//     let position_breakdown = cl
//         .query_position_by_id(&PositionByIdRequest { position_id })?
//         .position
//         .unwrap();
//     let position = position_breakdown.position.unwrap();

//     cl.withdraw_position(
//         MsgWithdrawPosition {
//             position_id: position.position_id,
//             sender: chain.sender().to_string(),
//             liquidity_amount: position.liquidity,
//         },
//         &chain.sender,
//     )?;

//     // Ensure it errors
//     carrot_app.withdraw_all().unwrap_err();

//     // Ensure position deleted
//     let position_not_found = cl
//         .query_position_by_id(&PositionByIdRequest { position_id })
//         .unwrap_err();
//     assert!(position_not_found
//         .to_string()
//         .contains("position not found"));
//     Ok(())
// }

// #[test]
// fn deposit_slippage() -> anyhow::Result<()> {
//     let (_, carrot_app) = setup_test_tube(false)?;

//     let deposit_amount = 5_000;
//     let max_fee = Uint128::new(deposit_amount).mul_floor(Decimal::percent(3));
//     // Create position
//     create_position(
//         &carrot_app,
//         coins(deposit_amount, USDT.to_owned()),
//         coin(1_000_000, USDT.to_owned()),
//         coin(1_000_000, USDC.to_owned()),
//     )?;

//     // Do the deposit of asset0 with incorrect belief_price1
//     let e = carrot_app
//         .deposit(
//             vec![coin(deposit_amount, USDT.to_owned())],
//             None,
//             Some(Decimal::zero()),
//             None,
//         )
//         .unwrap_err();
//     assert!(e.to_string().contains("exceeds max spread limit"));

//     // Do the deposit of asset1 with incorrect belief_price0
//     let e = carrot_app
//         .deposit(
//             vec![coin(deposit_amount, USDC.to_owned())],
//             Some(Decimal::zero()),
//             None,
//             None,
//         )
//         .unwrap_err();
//     assert!(e.to_string().contains("exceeds max spread limit"));

//     // Do the deposits of asset0 with correct belief_price
//     carrot_app.deposit(
//         vec![coin(deposit_amount, USDT.to_owned())],
//         None,
//         Some(Decimal::one()),
//         Some(Decimal::percent(10)),
//     )?;
//     // Do the deposits of asset1 with correct belief_price
//     carrot_app.deposit(
//         vec![coin(deposit_amount, USDT.to_owned())],
//         Some(Decimal::one()),
//         None,
//         Some(Decimal::percent(10)),
//     )?;

//     // Check almost everything landed
//     let balance: AssetsBalanceResponse = carrot_app.balance()?;
//     let sum = balance
//         .balances
//         .iter()
//         .fold(Uint128::zero(), |acc, e| acc + e.amount);
//     assert!(sum.u128() > (deposit_amount - max_fee.u128()) * 3);
//     Ok(())
// }
