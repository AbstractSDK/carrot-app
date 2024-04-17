mod common;

use crate::common::{deposit_with_funds, setup_test_tube, USDT};
use abstract_app::objects::AnsAsset;
use carrot_app::{helpers::close_to, msg::AppQueryMsgFns};
use cosmwasm_std::Decimal;
use cw_orch::anyhow;

#[test]
fn query_strategy_status() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    // We should add funds to the account proxy
    let deposit_amount = 5_000u128;

    // Do the deposit
    deposit_with_funds(&carrot_app, vec![AnsAsset::new(USDT, deposit_amount)])?;

    let strategy = carrot_app.strategy_status()?.strategy;

    assert_eq!(strategy.0.len(), 1);
    let single_strategy = strategy.0[0].clone();
    assert_eq!(single_strategy.share, Decimal::one());
    assert_eq!(single_strategy.yield_source.asset_distribution.len(), 2);
    // The strategy shares are a little off 50%
    assert_ne!(
        single_strategy.yield_source.asset_distribution[0].share,
        Decimal::percent(50)
    );
    assert_ne!(
        single_strategy.yield_source.asset_distribution[1].share,
        Decimal::percent(50)
    );
    assert!(close_to(
        Decimal::one(),
        single_strategy.yield_source.asset_distribution[0].share
            + single_strategy.yield_source.asset_distribution[1].share
    ),);

    Ok(())
}
