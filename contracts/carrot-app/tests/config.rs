mod common;

use crate::common::{create_pool, setup_test_tube, USDC, USDT};
use carrot_app::{
    msg::{AppExecuteMsgFns, AppQueryMsgFns},
    yield_sources::{
        osmosis_cl_pool::ConcentratedPoolParamsBase, yield_type::YieldTypeBase, AssetShare,
        StrategyBase, StrategyElementBase, YieldSourceBase,
    },
};
use common::{INITIAL_LOWER_TICK, INITIAL_UPPER_TICK};
use cosmwasm_std::{coins, Decimal, Uint128};
use cw_orch::anyhow;
use cw_orch::prelude::BankSetter;
use cw_orch::prelude::ContractInstance;

#[test]
fn rebalance_fails() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    carrot_app
        .update_strategy(
            vec![],
            StrategyBase(vec![
                StrategyElementBase {
                    yield_source: YieldSourceBase {
                        asset_distribution: vec![
                            AssetShare {
                                denom: USDT.to_string(),
                                share: Decimal::percent(50),
                            },
                            AssetShare {
                                denom: USDC.to_string(),
                                share: Decimal::percent(50),
                            },
                        ],
                        ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                            pool_id: 7,
                            lower_tick: INITIAL_LOWER_TICK,
                            upper_tick: INITIAL_UPPER_TICK,
                            position_id: None,
                            _phantom: std::marker::PhantomData,
                        }),
                    },
                    share: Decimal::one(),
                },
                StrategyElementBase {
                    yield_source: YieldSourceBase {
                        asset_distribution: vec![
                            AssetShare {
                                denom: USDT.to_string(),
                                share: Decimal::percent(50),
                            },
                            AssetShare {
                                denom: USDC.to_string(),
                                share: Decimal::percent(50),
                            },
                        ],
                        ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                            pool_id: 7,
                            lower_tick: INITIAL_LOWER_TICK,
                            upper_tick: INITIAL_UPPER_TICK,
                            position_id: None,
                            _phantom: std::marker::PhantomData,
                        }),
                    },
                    share: Decimal::one(),
                },
            ]),
        )
        .unwrap_err();

    // We query the nex strategy

    Ok(())
}

#[test]
fn rebalance_success() -> anyhow::Result<()> {
    let (pool_id, carrot_app) = setup_test_tube(false)?;
    let mut chain = carrot_app.get_chain().clone();

    let new_strat = StrategyBase(vec![
        StrategyElementBase {
            yield_source: YieldSourceBase {
                asset_distribution: vec![
                    AssetShare {
                        denom: USDT.to_string(),
                        share: Decimal::percent(50),
                    },
                    AssetShare {
                        denom: USDC.to_string(),
                        share: Decimal::percent(50),
                    },
                ],
                ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                    pool_id, // Pool Id needs to exist
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                    _phantom: std::marker::PhantomData,
                }),
            },
            share: Decimal::percent(50),
        },
        StrategyElementBase {
            yield_source: YieldSourceBase {
                asset_distribution: vec![
                    AssetShare {
                        denom: USDT.to_string(),
                        share: Decimal::percent(50),
                    },
                    AssetShare {
                        denom: USDC.to_string(),
                        share: Decimal::percent(50),
                    },
                ],
                ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                    pool_id, // Pool Id needs to exist
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                    _phantom: std::marker::PhantomData,
                }),
            },
            share: Decimal::percent(50),
        },
    ]);
    let strategy = carrot_app.strategy()?;
    assert_ne!(strategy.strategy, new_strat);
    let deposit_coins = coins(10, USDC);
    chain.add_balance(
        carrot_app.account().proxy()?.to_string(),
        deposit_coins.clone(),
    )?;

    carrot_app.update_strategy(deposit_coins, new_strat.clone())?;

    // We query the new strategy
    let strategy = carrot_app.strategy()?;
    assert_eq!(strategy.strategy.0.len(), 2);

    Ok(())
}

#[test]
fn rebalance_with_new_pool_success() -> anyhow::Result<()> {
    let (pool_id, carrot_app) = setup_test_tube(false)?;
    let mut chain = carrot_app.get_chain().clone();
    let (new_pool_id, _) = create_pool(chain.clone())?;

    let deposit_amount = 10_000;
    let deposit_coins = coins(deposit_amount, USDT);

    chain.add_balance(
        carrot_app.account().proxy()?.to_string(),
        deposit_coins.clone(),
    )?;

    let new_strat = StrategyBase(vec![
        StrategyElementBase {
            yield_source: YieldSourceBase {
                asset_distribution: vec![
                    AssetShare {
                        denom: USDT.to_string(),
                        share: Decimal::percent(50),
                    },
                    AssetShare {
                        denom: USDC.to_string(),
                        share: Decimal::percent(50),
                    },
                ],
                ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                    pool_id, // Pool Id needs to exist
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                    _phantom: std::marker::PhantomData,
                }),
            },
            share: Decimal::percent(50),
        },
        StrategyElementBase {
            yield_source: YieldSourceBase {
                asset_distribution: vec![
                    AssetShare {
                        denom: USDT.to_string(),
                        share: Decimal::percent(50),
                    },
                    AssetShare {
                        denom: USDC.to_string(),
                        share: Decimal::percent(50),
                    },
                ],
                ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                    pool_id: new_pool_id,
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                    _phantom: std::marker::PhantomData,
                }),
            },
            share: Decimal::percent(50),
        },
    ]);
    carrot_app.update_strategy(deposit_coins.clone(), new_strat.clone())?;

    carrot_app.strategy()?;

    // We query the balance
    let balance = carrot_app.balance()?;
    assert!(balance.total_value > Uint128::from(deposit_amount) * Decimal::percent(2));

    let distribution = carrot_app.positions()?;

    // We make sure the total values are close between the 2 positions
    let balance0 = distribution.positions[0].balance.total_value;
    let balance1 = distribution.positions[1].balance.total_value;
    let balance_diff = balance0
        .checked_sub(balance1)
        .or(balance1.checked_sub(balance0))?;
    assert!(balance_diff < Uint128::from(deposit_amount) * Decimal::permille(5));

    Ok(())
}

#[test]
fn rebalance_with_stale_strategy_success() -> anyhow::Result<()> {
    let (pool_id, carrot_app) = setup_test_tube(false)?;
    let mut chain = carrot_app.get_chain().clone();
    let (new_pool_id, _) = create_pool(chain.clone())?;

    let deposit_amount = 10_000;
    let deposit_coins = coins(deposit_amount, USDT);

    chain.add_balance(
        carrot_app.account().proxy()?.to_string(),
        deposit_coins.clone(),
    )?;
    let common_yield_source = YieldSourceBase {
        asset_distribution: vec![
            AssetShare {
                denom: USDT.to_string(),
                share: Decimal::percent(50),
            },
            AssetShare {
                denom: USDC.to_string(),
                share: Decimal::percent(50),
            },
        ],
        ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
            pool_id, // Pool Id needs to exist
            lower_tick: INITIAL_LOWER_TICK,
            upper_tick: INITIAL_UPPER_TICK,
            position_id: None,
            _phantom: std::marker::PhantomData,
        }),
    };

    let strat = StrategyBase(vec![
        StrategyElementBase {
            yield_source: common_yield_source.clone(),
            share: Decimal::percent(50),
        },
        StrategyElementBase {
            yield_source: YieldSourceBase {
                asset_distribution: vec![
                    AssetShare {
                        denom: USDT.to_string(),
                        share: Decimal::percent(50),
                    },
                    AssetShare {
                        denom: USDC.to_string(),
                        share: Decimal::percent(50),
                    },
                ],
                ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                    pool_id: new_pool_id,
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                    _phantom: std::marker::PhantomData,
                }),
            },
            share: Decimal::percent(50),
        },
    ]);

    carrot_app.update_strategy(deposit_coins.clone(), strat.clone())?;

    let new_strat = StrategyBase(vec![StrategyElementBase {
        yield_source: common_yield_source.clone(),
        share: Decimal::percent(100),
    }]);
    let total_value_before = carrot_app.balance()?.total_value;

    // No additional deposit
    carrot_app.update_strategy(vec![], new_strat.clone())?;

    carrot_app.strategy()?;

    // We query the balance
    let balance = carrot_app.balance()?;
    // Make sure the deposit went almost all in
    assert!(balance.total_value > Uint128::from(deposit_amount) * Decimal::percent(98));
    println!(
        "Before :{}, after: {}",
        total_value_before, balance.total_value
    );
    // Make sure the total value has almost not changed when updating the strategy
    assert!(balance.total_value > total_value_before * Decimal::permille(999));

    let distribution = carrot_app.positions()?;

    // We make sure the total values are close between the 2 positions
    assert_eq!(distribution.positions.len(), 1);

    Ok(())
}

#[test]
fn rebalance_with_current_and_stale_strategy_success() -> anyhow::Result<()> {
    let (pool_id, carrot_app) = setup_test_tube(false)?;
    let mut chain = carrot_app.get_chain().clone();
    let (new_pool_id, _) = create_pool(chain.clone())?;

    let deposit_amount = 10_000;
    let deposit_coins = coins(deposit_amount, USDT);

    chain.add_balance(
        carrot_app.account().proxy()?.to_string(),
        deposit_coins.clone(),
    )?;
    let moving_strategy = YieldSourceBase {
        asset_distribution: vec![
            AssetShare {
                denom: USDT.to_string(),
                share: Decimal::percent(50),
            },
            AssetShare {
                denom: USDC.to_string(),
                share: Decimal::percent(50),
            },
        ],
        ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
            pool_id: new_pool_id,
            lower_tick: INITIAL_LOWER_TICK,
            upper_tick: INITIAL_UPPER_TICK,
            position_id: None,
            _phantom: std::marker::PhantomData,
        }),
    };

    let strat = StrategyBase(vec![
        StrategyElementBase {
            yield_source: YieldSourceBase {
                asset_distribution: vec![
                    AssetShare {
                        denom: USDT.to_string(),
                        share: Decimal::percent(50),
                    },
                    AssetShare {
                        denom: USDC.to_string(),
                        share: Decimal::percent(50),
                    },
                ],
                ty: YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                    pool_id, // Pool Id needs to exist
                    lower_tick: INITIAL_LOWER_TICK,
                    upper_tick: INITIAL_UPPER_TICK,
                    position_id: None,
                    _phantom: std::marker::PhantomData,
                }),
            },
            share: Decimal::percent(50),
        },
        StrategyElementBase {
            yield_source: moving_strategy.clone(),
            share: Decimal::percent(50),
        },
    ]);

    carrot_app.update_strategy(deposit_coins.clone(), strat.clone())?;

    let mut strategies = carrot_app.strategy()?.strategy;

    strategies.0[1].yield_source = moving_strategy;

    let total_value_before = carrot_app.balance()?.total_value;

    // No additional deposit
    carrot_app.update_strategy(vec![], strategies.clone())?;

    carrot_app.strategy()?;

    // We query the balance
    let balance = carrot_app.balance()?;
    // Make sure the deposit went almost all in
    assert!(balance.total_value > Uint128::from(deposit_amount) * Decimal::percent(98));
    println!(
        "Before :{}, after: {}",
        total_value_before, balance.total_value
    );
    // Make sure the total value has almost not changed when updating the strategy
    assert!(balance.total_value > total_value_before * Decimal::permille(998));

    let distribution = carrot_app.positions()?;

    // We make sure the total values are close between the 2 positions
    assert_eq!(distribution.positions.len(), 2);

    Ok(())
}
