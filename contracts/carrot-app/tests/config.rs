mod common;

use crate::common::{setup_test_tube, USDC, USDT};
use carrot_app::{
    msg::{AppExecuteMsgFns, AppQueryMsgFns},
    yield_sources::{
        yield_type::{ConcentratedPoolParams, YieldType},
        BalanceStrategy, BalanceStrategyElement, YieldSource,
    },
};
use common::{INITIAL_LOWER_TICK, INITIAL_UPPER_TICK};
use cosmwasm_std::Decimal;
use cw_orch::{anyhow, prelude::*};

#[test]
fn rebalance_fails() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    carrot_app
        .rebalance(BalanceStrategy(vec![
            BalanceStrategyElement {
                yield_source: YieldSource {
                    expected_tokens: vec![
                        (USDT.to_string(), Decimal::percent(50)),
                        (USDC.to_string(), Decimal::percent(50)),
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
                    expected_tokens: vec![
                        (USDT.to_string(), Decimal::percent(50)),
                        (USDC.to_string(), Decimal::percent(50)),
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
    let (_, carrot_app) = setup_test_tube(false)?;

    let new_strat = BalanceStrategy(vec![
        BalanceStrategyElement {
            yield_source: YieldSource {
                expected_tokens: vec![
                    (USDT.to_string(), Decimal::percent(50)),
                    (USDC.to_string(), Decimal::percent(50)),
                ],
                ty: YieldType::ConcentratedLiquidityPool(ConcentratedPoolParams {
                    pool_id: 7,
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                }),
            },
            share: Decimal::percent(50),
        },
        BalanceStrategyElement {
            yield_source: YieldSource {
                expected_tokens: vec![
                    (USDT.to_string(), Decimal::percent(50)),
                    (USDC.to_string(), Decimal::percent(50)),
                ],
                ty: YieldType::ConcentratedLiquidityPool(ConcentratedPoolParams {
                    pool_id: 7,
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                }),
            },
            share: Decimal::percent(50),
        },
    ]);
    carrot_app.rebalance(new_strat.clone())?;

    let strategy = carrot_app.strategy()?;

    assert_eq!(strategy.strategy, new_strat);

    // We query the nex strategy

    Ok(())
}
