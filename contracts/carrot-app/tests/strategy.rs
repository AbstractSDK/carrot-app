use std::collections::HashMap;

use carrot_app::yield_sources::yield_type::YieldType;
use cosmwasm_std::{coin, Decimal};

use carrot_app::state::compute_total_value;
use carrot_app::yield_sources::{BalanceStrategy, BalanceStrategyElement, YieldSource};

pub const LUNA: &str = "uluna";
pub const OSMOSIS: &str = "uosmo";
pub const STARGAZE: &str = "ustars";
pub const NEUTRON: &str = "untrn";
pub const USD: &str = "usd";

pub fn mock_strategy() -> BalanceStrategy {
    BalanceStrategy(vec![
        BalanceStrategyElement {
            yield_source: YieldSource {
                expected_tokens: vec![
                    (LUNA.to_string(), Decimal::percent(30)),
                    (OSMOSIS.to_string(), Decimal::percent(10)),
                    (STARGAZE.to_string(), Decimal::percent(60)),
                ],
                ty: YieldType::Mars("usdc".to_string()),
            },
            share: Decimal::percent(33),
        },
        BalanceStrategyElement {
            yield_source: YieldSource {
                expected_tokens: vec![(NEUTRON.to_string(), Decimal::percent(100))],
                ty: YieldType::Mars("usdc".to_string()),
            },
            share: Decimal::percent(67),
        },
    ])
}

#[test]
fn bad_strategy_check_empty() -> cw_orch::anyhow::Result<()> {
    let strategy = BalanceStrategy(vec![
        BalanceStrategyElement {
            yield_source: YieldSource {
                expected_tokens: vec![],
                ty: YieldType::Mars("usdc".to_string()),
            },
            share: Decimal::percent(33),
        },
        BalanceStrategyElement {
            yield_source: YieldSource {
                expected_tokens: vec![],
                ty: YieldType::Mars("usdc".to_string()),
            },
            share: Decimal::percent(67),
        },
    ]);

    strategy.check().unwrap_err();

    Ok(())
}

#[test]
fn bad_strategy_check_sum() -> cw_orch::anyhow::Result<()> {
    let strategy = BalanceStrategy(vec![
        BalanceStrategyElement {
            yield_source: YieldSource {
                expected_tokens: vec![(NEUTRON.to_string(), Decimal::percent(100))],
                ty: YieldType::Mars("usdc".to_string()),
            },
            share: Decimal::percent(33),
        },
        BalanceStrategyElement {
            yield_source: YieldSource {
                expected_tokens: vec![(NEUTRON.to_string(), Decimal::percent(100))],
                ty: YieldType::Mars("usdc".to_string()),
            },
            share: Decimal::percent(66),
        },
    ]);

    strategy.check().unwrap_err();

    Ok(())
}

#[test]
fn bad_strategy_check_sum_inner() -> cw_orch::anyhow::Result<()> {
    let strategy = BalanceStrategy(vec![
        BalanceStrategyElement {
            yield_source: YieldSource {
                expected_tokens: vec![
                    (NEUTRON.to_string(), Decimal::percent(33)),
                    (NEUTRON.to_string(), Decimal::percent(66)),
                ],
                ty: YieldType::Mars("usdc".to_string()),
            },
            share: Decimal::percent(33),
        },
        BalanceStrategyElement {
            yield_source: YieldSource {
                expected_tokens: vec![(NEUTRON.to_string(), Decimal::percent(100))],
                ty: YieldType::Mars("usdc".to_string()),
            },
            share: Decimal::percent(67),
        },
    ]);

    strategy.check().unwrap_err();

    Ok(())
}

#[test]
fn check_strategy() -> cw_orch::anyhow::Result<()> {
    let strategy = mock_strategy();

    strategy.check()?;

    Ok(())
}

#[test]
fn value_fill_strategy() -> cw_orch::anyhow::Result<()> {
    let strategy = mock_strategy();

    let exchange_rates: HashMap<String, Decimal> = [
        (LUNA.to_string(), Decimal::percent(150)),
        (USD.to_string(), Decimal::percent(100)),
        (NEUTRON.to_string(), Decimal::percent(75)),
        (OSMOSIS.to_string(), Decimal::percent(10)),
        (STARGAZE.to_string(), Decimal::percent(35)),
    ]
    .into_iter()
    .collect();

    let funds = vec![
        coin(1_000_000_000, LUNA),
        coin(2_000_000_000, USD),
        coin(25_000_000, NEUTRON),
    ];
    println!(
        "total value : {:?}",
        compute_total_value(&funds, &exchange_rates)
    );

    let fill_result = strategy.fill_all(funds, &exchange_rates)?;

    assert_eq!(fill_result.len(), 2);
    assert_eq!(fill_result[0].0.len(), 3);
    assert_eq!(fill_result[1].0.len(), 1);
    Ok(())
}
