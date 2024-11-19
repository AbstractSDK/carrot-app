mod common;

use std::str::FromStr;

use crate::common::{create_position, setup_test_tube, USDC, USDC_DENOM, USDT, USDT_DENOM};
use abstract_app::objects::AssetEntry;
use abstract_interface::{Abstract, AbstractAccount};
use carrot_app::msg::{
    AppExecuteMsg, AppExecuteMsgFns, AppQueryMsgFns, AssetsBalanceResponse, CompoundStatus,
    CreatePositionMessage, PositionResponse, SwapToAsset,
};
use common::{DEX_NAME, GAS_DENOM};
use cosmwasm_std::{coin, coins, Decimal, Uint128, Uint256};
use cw_orch::anyhow;
use cw_orch::prelude::*;
use cw_orch_osmosis_test_tube::osmosis_test_tube::osmosis_std::types::osmosis::concentratedliquidity::v1beta1::PositionByIdRequest;
use cw_orch_osmosis_test_tube::osmosis_test_tube::{
    osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgWithdrawPosition,
    ConcentratedLiquidity, Module,
};

#[test]
fn deposit_lands() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let deposit_amount = 5_000;
    // Either missed position range or fees
    let max_difference = Uint128::new(deposit_amount).mul_floor(Decimal::percent(3));
    // Create position
    create_position(
        &carrot_app,
        coins(deposit_amount, USDT_DENOM.to_owned()),
        coin(1_000_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDC_DENOM.to_owned()),
    )?;
    // Check almost everything landed
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > deposit_amount - max_difference.u128());

    // Do the deposit
    carrot_app.deposit(
        vec![coin(deposit_amount, USDT_DENOM.to_owned())],
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
    assert!(sum.u128() > (deposit_amount - max_difference.u128()) * 2);

    // Do the second deposit
    carrot_app.deposit(
        vec![coin(deposit_amount, USDT_DENOM.to_owned())],
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
    assert!(sum.u128() > (deposit_amount - max_difference.u128()) * 3);
    Ok(())
}

#[test]
fn withdraw_position() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.environment().clone();

    // Create position
    create_position(
        &carrot_app,
        coins(10_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDC_DENOM.to_owned()),
    )?;

    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let balance_usdc_before_withdraw = chain
        .bank_querier()
        .balance(chain.sender_addr(), Some(USDT_DENOM.to_owned()))?
        .pop()
        .unwrap();
    let balance_usdt_before_withdraw = chain
        .bank_querier()
        .balance(chain.sender_addr(), Some(USDC_DENOM.to_owned()))?
        .pop()
        .unwrap();

    // Withdraw half of liquidity
    let liquidity_amount: Uint256 = balance.liquidity.parse().unwrap();
    let half_of_liquidity = liquidity_amount / Uint256::from_u128(2);
    carrot_app.withdraw(Some(half_of_liquidity), None)?;

    let balance_usdc_after_half_withdraw = chain
        .bank_querier()
        .balance(chain.sender_addr(), Some(USDT_DENOM.to_owned()))?
        .pop()
        .unwrap();
    let balance_usdt_after_half_withdraw = chain
        .bank_querier()
        .balance(chain.sender_addr(), Some(USDC_DENOM.to_owned()))?
        .pop()
        .unwrap();

    assert!(balance_usdc_after_half_withdraw.amount > balance_usdc_before_withdraw.amount);
    assert!(balance_usdt_after_half_withdraw.amount > balance_usdt_before_withdraw.amount);

    // Withdraw rest of liquidity
    carrot_app.withdraw(None, None)?;
    let balance_usdc_after_full_withdraw = chain
        .bank_querier()
        .balance(chain.sender_addr(), Some(USDT_DENOM.to_owned()))?
        .pop()
        .unwrap();
    let balance_usdt_after_full_withdraw = chain
        .bank_querier()
        .balance(chain.sender_addr(), Some(USDC_DENOM.to_owned()))?
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
        coins(10_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDC_DENOM.to_owned()),
    )?;

    carrot_app.deposit(
        vec![
            coin(258, USDT_DENOM.to_owned()),
            coin(234, USDC_DENOM.to_owned()),
        ],
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
    assert!(position.position_id.is_some());
    Ok(())
}

#[test]
fn withdraw_after_user_withdraw_liquidity_manually() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(true)?;
    let chain = carrot_app.environment().clone();

    let position: PositionResponse = carrot_app.position()?;
    let position_id = position.position_id.unwrap();

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
            sender: chain.sender_addr().to_string(),
            liquidity_amount: position.liquidity,
        },
        &chain.sender,
    )?;

    // Ensure it errors
    carrot_app.withdraw(None, None).unwrap_err();

    // Ensure we get correct compound response
    let status_response = carrot_app.compound_status()?;
    assert_eq!(
        status_response.status,
        CompoundStatus::PositionNotAvailable(position_id)
    );

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
    let max_difference = Uint128::new(deposit_amount).mul_floor(Decimal::percent(3));
    // Create position
    create_position(
        &carrot_app,
        coins(deposit_amount, USDT_DENOM.to_owned()),
        coin(1_000_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDC_DENOM.to_owned()),
    )?;

    // Do the deposit of asset0 with incorrect belief_price1
    let e = carrot_app
        .deposit(
            vec![coin(deposit_amount, USDT_DENOM.to_owned())],
            None,
            Some(Decimal::zero()),
            None,
        )
        .unwrap_err();
    assert!(e.to_string().contains("exceeds max spread limit"));

    // Do the deposit of asset1 with incorrect belief_price0
    let e = carrot_app
        .deposit(
            vec![coin(deposit_amount, USDC_DENOM.to_owned())],
            Some(Decimal::zero()),
            None,
            None,
        )
        .unwrap_err();
    assert!(e.to_string().contains("exceeds max spread limit"));

    // Do the deposits of asset0 with correct belief_price
    carrot_app.deposit(
        vec![coin(deposit_amount, USDT_DENOM.to_owned())],
        None,
        Some(Decimal::one()),
        Some(Decimal::percent(10)),
    )?;
    // Do the deposits of asset1 with correct belief_price
    carrot_app.deposit(
        vec![coin(deposit_amount, USDT_DENOM.to_owned())],
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
    assert!(sum.u128() > (deposit_amount - max_difference.u128()) * 3);
    Ok(())
}

#[test]
fn partial_withdraw_position_autoclaims() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.environment().clone();
    // Create position
    create_position(
        &carrot_app,
        coins(10_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDC_DENOM.to_owned()),
    )?;

    // Do some swaps
    let dex: abstract_dex_adapter::interface::DexAdapter<_> = carrot_app.module()?;
    let abs = Abstract::load_from(chain.clone())?;
    let account_id = carrot_app.account().id()?;
    let account = AbstractAccount::new(&abs, account_id);
    chain.bank_send(
        account.proxy.addr_str()?,
        vec![
            coin(200_000, USDC_DENOM.to_owned()),
            coin(200_000, USDT_DENOM.to_owned()),
        ],
    )?;
    for _ in 0..10 {
        dex.ans_swap(
            (USDC, 50_000),
            USDT,
            DEX_NAME.to_string(),
            &account,
            &abs.ans_host,
        )?;
        dex.ans_swap(
            (USDT, 50_000),
            USDC,
            DEX_NAME.to_string(),
            &account,
            &abs.ans_host,
        )?;
    }

    // Check it has some rewards
    let status = carrot_app.compound_status()?;
    assert!(!status.spread_rewards.is_empty());

    // Withdraw half of liquidity
    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let liquidity_amount: Uint256 = balance.liquidity.parse().unwrap();
    let half_of_liquidity = liquidity_amount / Uint256::from_u128(2);
    carrot_app.withdraw(Some(half_of_liquidity), None)?;

    // Check rewards claimed
    let status = carrot_app.compound_status()?;
    assert!(status.spread_rewards.is_empty());

    Ok(())
}

#[test]
fn manual_partial_withdraw_position_doesnt_autoclaim() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.environment().clone();

    // Create position
    create_position(
        &carrot_app,
        coins(10_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDC_DENOM.to_owned()),
    )?;

    // Do some swaps
    let dex: abstract_dex_adapter::interface::DexAdapter<_> = carrot_app.module()?;
    let abs = Abstract::load_from(chain.clone())?;
    let account_id = carrot_app.account().id()?;
    let account = AbstractAccount::new(&abs, account_id);
    chain.bank_send(
        account.proxy.addr_str()?,
        vec![
            coin(200_000, USDC_DENOM.to_owned()),
            coin(200_000, USDT_DENOM.to_owned()),
        ],
    )?;
    for _ in 0..10 {
        dex.ans_swap(
            (USDC, 50_000),
            USDT,
            DEX_NAME.to_string(),
            &account,
            &abs.ans_host,
        )?;
        dex.ans_swap(
            (USDT, 50_000),
            USDC,
            DEX_NAME.to_string(),
            &account,
            &abs.ans_host,
        )?;
    }

    // Check it has some rewards
    let status = carrot_app.compound_status()?;
    assert!(!status.spread_rewards.is_empty());

    // Withdraw half of liquidity
    let test_tube = chain.app.borrow();
    let cl = ConcentratedLiquidity::new(&*test_tube);

    let position: PositionResponse = carrot_app.position()?;
    let position_id = position.position_id.unwrap();

    let balance: AssetsBalanceResponse = carrot_app.balance()?;
    let liquidity_amount: Uint128 = balance.liquidity.parse().unwrap();
    let half_of_liquidity = liquidity_amount / Uint128::new(2);

    cl.withdraw_position(
        MsgWithdrawPosition {
            position_id,
            sender: chain.sender_addr().to_string(),
            liquidity_amount: half_of_liquidity.to_string(),
        },
        &chain.sender,
    )?;

    // Check rewards not claimed
    let status = carrot_app.compound_status()?;
    assert!(!status.spread_rewards.is_empty());

    Ok(())
}

#[test]
fn shifted_position_create_deposit() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let deposit_amount = 10_000;
    let max_difference = Uint128::new(deposit_amount).mul_floor(Decimal::percent(3));

    // Create one-way shifted position
    carrot_app.create_position(CreatePositionMessage {
        lower_tick: -37000,
        upper_tick: 1000,
        funds: coins(deposit_amount, USDT_DENOM),
        asset0: coin(205_000, USDT_DENOM),
        asset1: coin(753_000, USDC_DENOM),
        max_spread: None,
        belief_price0: None,
        belief_price1: None,
    })?;

    let balance = carrot_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > (deposit_amount - max_difference.u128()));

    // Deposit asset0
    carrot_app.deposit(coins(deposit_amount, USDT_DENOM), None, None, None)?;
    let balance = carrot_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > (deposit_amount - max_difference.u128()) * 2);

    // Deposit asset1
    carrot_app.deposit(coins(deposit_amount, USDT_DENOM), None, None, None)?;
    let balance = carrot_app.balance()?;
    let sum = balance
        .balances
        .iter()
        .fold(Uint128::zero(), |acc, e| acc + e.amount);
    assert!(sum.u128() > (deposit_amount - max_difference.u128()) * 3);
    Ok(())
}

#[test]
fn error_on_provided_funds() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    carrot_app
        .execute(
            &AppExecuteMsg::CreatePosition(CreatePositionMessage {
                lower_tick: -37000,
                upper_tick: 1000,
                funds: coins(10_000, USDT),
                asset0: coin(205_000, USDT),
                asset1: coin(753_000, USDC),
                max_spread: None,
                belief_price0: None,
                belief_price1: None,
            })
            .into(),
            Some(&[coin(10, GAS_DENOM)]),
        )
        .expect_err("Should error when funds provided");
    Ok(())
}

#[test]
fn withdraw_to_asset() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.environment().clone();
    let initial_amount = 100_000;
    // Create position
    create_position(
        &carrot_app,
        coins(initial_amount, USDT_DENOM.to_owned()),
        coin(1_000_000, USDT_DENOM.to_owned()),
        coin(1_000_000, USDC_DENOM.to_owned()),
    )?;
    let liquidity = carrot_app.balance()?.liquidity;
    let liquidity = Uint256::from_str(&liquidity)?;
    let withdraw_liquidity_amount = liquidity / Uint256::from_u128(3);
    let withdraw_amount = Uint128::new(initial_amount / 3);
    let max_fee = withdraw_amount.mul_floor(Decimal::percent(3));

    // Withdraw to asset1
    {
        let asset0_balance_before = chain
            .bank_querier()
            .balance(chain.sender_addr(), Some(USDT_DENOM.to_owned()))?;
        let asset1_balance_before = chain
            .bank_querier()
            .balance(chain.sender_addr(), Some(USDC_DENOM.to_owned()))?;

        carrot_app.withdraw(
            Some(withdraw_liquidity_amount),
            Some(carrot_app::msg::SwapToAsset {
                to_asset: AssetEntry::new(USDC),
                max_spread: None,
            }),
        )?;

        let asset0_balance_after = chain
            .bank_querier()
            .balance(chain.sender_addr(), Some(USDT_DENOM.to_owned()))?;
        let asset1_balance_after = chain
            .bank_querier()
            .balance(chain.sender_addr(), Some(USDC_DENOM.to_owned()))?;

        assert_eq!(asset0_balance_before, asset0_balance_after);
        assert!(
            asset1_balance_after[0].amount - asset1_balance_before[0].amount
                >= withdraw_amount - max_fee
        );
    }

    // Withdraw to asset0
    {
        let asset0_balance_before = chain
            .bank_querier()
            .balance(chain.sender_addr(), Some(USDT_DENOM.to_owned()))?;
        let asset1_balance_before = chain
            .bank_querier()
            .balance(chain.sender_addr(), Some(USDC_DENOM.to_owned()))?;

        carrot_app.withdraw(
            Some(withdraw_liquidity_amount),
            Some(SwapToAsset {
                to_asset: AssetEntry::new(USDT),
                max_spread: None,
            }),
        )?;

        let asset0_balance_after = chain
            .bank_querier()
            .balance(chain.sender_addr(), Some(USDT_DENOM.to_owned()))?;
        let asset1_balance_after = chain
            .bank_querier()
            .balance(chain.sender_addr(), Some(USDC_DENOM.to_owned()))?;

        assert_eq!(asset1_balance_before, asset1_balance_after);
        assert!(
            asset0_balance_after[0].amount - asset0_balance_before[0].amount
                >= withdraw_amount - max_fee
        );
    }
    Ok(())
}
