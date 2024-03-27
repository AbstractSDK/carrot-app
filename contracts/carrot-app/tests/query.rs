mod common;

use crate::common::{setup_test_tube, USDT};
use carrot_app::{
    helpers::close_to,
    msg::{AppExecuteMsgFns, AppQueryMsgFns},
};
use cosmwasm_std::{coins, Decimal};
use cw_orch::{anyhow, prelude::*};

#[test]
fn query_strategy_status() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    // We should add funds to the account proxy
    let deposit_amount = 5_000;
    let deposit_coins = coins(deposit_amount, USDT.to_owned());
    let mut chain = carrot_app.get_chain().clone();

    chain.add_balance(
        carrot_app.account().proxy()?.to_string(),
        deposit_coins.clone(),
    )?;

    // Do the deposit
    carrot_app.deposit(deposit_coins.clone(), None)?;

    let strategy = carrot_app.strategy_status()?.strategy;

    assert_eq!(strategy.0.len(), 1);
    let single_strategy = strategy.0[0].clone();
    assert_eq!(single_strategy.share, Decimal::one());
    assert_eq!(single_strategy.yield_source.expected_tokens.len(), 2);
    // The strategy shares are a little off 50%
    assert_ne!(
        single_strategy.yield_source.expected_tokens[0].share,
        Decimal::percent(50)
    );
    assert_ne!(
        single_strategy.yield_source.expected_tokens[1].share,
        Decimal::percent(50)
    );
    assert!(close_to(
        Decimal::one(),
        single_strategy.yield_source.expected_tokens[0].share
            + single_strategy.yield_source.expected_tokens[1].share
    ),);

    Ok(())
}
