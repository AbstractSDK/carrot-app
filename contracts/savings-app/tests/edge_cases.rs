mod interface;
use interface::*;

use app::msg::{AppQueryMsgFns, AssetsBalanceResponse};
use cosmwasm_std::{coin, coins, Decimal, Uint128};
use cw_orch::{anyhow, prelude::*};

use crate::interface::setup_test_tube;
#[test]
fn deposit_twice() -> anyhow::Result<()> {
    let (_, savings_app) = setup_test_tube(false)?;

    let chain = savings_app.get_chain().clone();

    let deposit_amount = 5_000;
    let max_fee = Uint128::new(deposit_amount).mul_floor(Decimal::percent(2));
    // Create position
    create_position(
        &savings_app,
        coins(deposit_amount, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = savings_app.balance()?;
    let first_sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(first_sum.u128() > deposit_amount - max_fee.u128());

    // Create a position once again
    create_position(
        &savings_app,
        coins(deposit_amount, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDC)),
        coin(1_000_000, factory_denom(&chain, USDT)),
    )?;

    // Check almost everything landed
    let balance: AssetsBalanceResponse = savings_app.balance()?;
    let second_sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!((second_sum - first_sum).u128() > deposit_amount - max_fee.u128());

    Ok(())
}
