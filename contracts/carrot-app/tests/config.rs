mod common;

use crate::common::{setup_test_tube, USDC, USDT};
use carrot_app::{
    msg::{AppExecuteMsgFns, AppQueryMsgFns},
    yield_sources::{
        yield_type::{ConcentratedPoolParams, YieldType},
        BalanceStrategy, BalanceStrategyElement, ExpectedToken, YieldSource,
    },
};
use common::{INITIAL_LOWER_TICK, INITIAL_UPPER_TICK};
use cosmwasm_std::Decimal;
use cw_orch::anyhow;

#[test]
fn rebalance_fails() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    carrot_app
        .update_strategy(BalanceStrategy(vec![
            BalanceStrategyElement {
                yield_source: YieldSource {
                    asset_distribution: vec![
                        ExpectedToken {
                            denom: USDT.to_string(),
                            share: Decimal::percent(50),
                        },
                        ExpectedToken {
                            denom: USDC.to_string(),
                            share: Decimal::percent(50),
                        },
                    ],
                    ty: YieldType::ConcentratedLiquidityPool(ConcentratedPoolParams {
                        pool_id: 7,
                        lower_tick: INITIAL_LOWER_TICK,
                        upper_tick: INITIAL_UPPER_TICK,
                        position_id: None,
                    }),
                },
                share: Decimal::one(),
            },
            BalanceStrategyElement {
                yield_source: YieldSource {
                    asset_distribution: vec![
                        ExpectedToken {
                            denom: USDT.to_string(),
                            share: Decimal::percent(50),
                        },
                        ExpectedToken {
                            denom: USDC.to_string(),
                            share: Decimal::percent(50),
                        },
                    ],
                    ty: YieldType::ConcentratedLiquidityPool(ConcentratedPoolParams {
                        pool_id: 7,
                        lower_tick: INITIAL_LOWER_TICK,
                        upper_tick: INITIAL_UPPER_TICK,
                        position_id: None,
                    }),
                },
                share: Decimal::one(),
            },
        ]))
        .unwrap_err();

    // We query the nex strategy

    Ok(())
}

#[test]
fn rebalance_success() -> anyhow::Result<()> {
    let (pool_id, carrot_app) = setup_test_tube(false)?;

    let new_strat = BalanceStrategy(vec![
        BalanceStrategyElement {
            yield_source: YieldSource {
                asset_distribution: vec![
                    ExpectedToken {
                        denom: USDT.to_string(),
                        share: Decimal::percent(50),
                    },
                    ExpectedToken {
                        denom: USDC.to_string(),
                        share: Decimal::percent(50),
                    },
                ],
                ty: YieldType::ConcentratedLiquidityPool(ConcentratedPoolParams {
                    pool_id, // Pool Id needs to exist
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                }),
            },
            share: Decimal::percent(50),
        },
        BalanceStrategyElement {
            yield_source: YieldSource {
                asset_distribution: vec![
                    ExpectedToken {
                        denom: USDT.to_string(),
                        share: Decimal::percent(50),
                    },
                    ExpectedToken {
                        denom: USDC.to_string(),
                        share: Decimal::percent(50),
                    },
                ],
                ty: YieldType::ConcentratedLiquidityPool(ConcentratedPoolParams {
                    pool_id, // Pool Id needs to exist
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                }),
            },
            share: Decimal::percent(50),
        },
    ]);
    carrot_app.update_strategy(new_strat.clone())?;

    // We query the new strategy
    let strategy = carrot_app.strategy()?;
    assert_eq!(strategy.strategy, new_strat);

    Ok(())
}
