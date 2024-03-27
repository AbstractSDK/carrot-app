mod common;

use crate::common::incentives::Incentives;
use crate::common::{
    create_position, setup_test_tube, DEX_NAME, GAS_DENOM, LOTS, REWARD_DENOM, USDC, USDT,
};
use abstract_app::abstract_interface::{Abstract, AbstractAccount};
use carrot_app::msg::{
    AppExecuteMsgFns, AppQueryMsgFns, AssetsBalanceResponse, CompoundStatus, CompoundStatusResponse,
};
use cosmwasm_std::{coin, coins, Uint128};
use cw_asset::AssetBase;
use cw_orch::osmosis_test_tube::osmosis_test_tube::{Account, Module};
use cw_orch::{anyhow, prelude::*};
use osmosis_std::shim::Timestamp;
use osmosis_std::types::cosmos::base::v1beta1;
use osmosis_std::types::osmosis::incentives::MsgCreateGauge;
use osmosis_std::types::osmosis::lockup::{LockQueryType, QueryCondition};

#[test]
fn check_autocompound() -> anyhow::Result<()> {
    let (pool_id, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.get_chain().clone();

    // Add incentive
    {
        let test_tube = chain.app.borrow();
        let time = test_tube.get_block_timestamp().plus_seconds(5);
        let incentives = Incentives::new(&*test_tube);
        let _ = incentives.create_gauge(
            MsgCreateGauge {
                is_perpetual: false,
                owner: chain.sender.address(),
                distribute_to: Some(QueryCondition {
                    lock_query_type: LockQueryType::NoLock.into(),
                    denom: "".to_owned(),
                    duration: None,
                    timestamp: None,
                }),
                coins: vec![v1beta1::Coin {
                    denom: GAS_DENOM.to_owned(),
                    amount: "100000000".to_owned(),
                }],
                start_time: Some(Timestamp {
                    seconds: time.seconds() as i64,
                    nanos: time.subsec_nanos() as i32,
                }),
                num_epochs_paid_over: 10,
                pool_id,
            },
            &chain.sender,
        )?;
    }
    // Create position
    create_position(
        &carrot_app,
        coins(100_000, USDT.to_owned()),
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

    // Check autocompound adds liquidity from the rewards and user balance remain unchanged

    // Check it has some rewards to autocompound first
    let rewards = carrot_app.compound_status()?.pool_rewards;
    assert!(rewards.iter().any(|c| c.denom == GAS_DENOM));

    // Save balances
    let balance_before_autocompound: AssetsBalanceResponse = carrot_app.balance()?;
    let balance_usdc_before_autocompound = chain
        .bank_querier()
        .balance(chain.sender(), Some(USDC.to_owned()))?
        .pop()
        .unwrap();
    let balance_usdt_before_autocompound = chain
        .bank_querier()
        .balance(chain.sender(), Some(USDT.to_owned()))?
        .pop()
        .unwrap();

    // Autocompound
    chain.wait_seconds(300)?;
    carrot_app.autocompound()?;

    // Save new balances
    let balance_after_autocompound: AssetsBalanceResponse = carrot_app.balance()?;
    let balance_usdc_after_autocompound = chain
        .bank_querier()
        .balance(chain.sender(), Some(USDC.to_owned()))?
        .pop()
        .unwrap();
    let balance_usdt_after_autocompound = chain
        .bank_querier()
        .balance(chain.sender(), Some(USDT.to_owned()))?
        .pop()
        .unwrap();

    // Liquidity added
    assert!(balance_after_autocompound.liquidity > balance_before_autocompound.liquidity);
    // Only rewards went in there
    assert!(balance_usdc_after_autocompound.amount >= balance_usdc_before_autocompound.amount);
    assert!(balance_usdt_after_autocompound.amount >= balance_usdt_before_autocompound.amount,);
    // Check it used all of the rewards
    let rewards = carrot_app.compound_status()?.pool_rewards;
    assert!(rewards.is_empty());

    Ok(())
}

#[test]
fn stranger_autocompound() -> anyhow::Result<()> {
    let (pool_id, carrot_app) = setup_test_tube(false)?;

    let mut chain = carrot_app.get_chain().clone();
    let stranger = chain.init_account(coins(LOTS, GAS_DENOM))?;

    // Create position
    create_position(
        &carrot_app,
        coins(100_000, USDT.to_owned()),
        coin(1_000_000, USDT.to_owned()),
        coin(1_000_000, USDC.to_owned()),
    )?;

    // Add incentive
    {
        let test_tube = chain.app.borrow();
        let time = test_tube.get_block_timestamp().plus_seconds(5);
        let incentives = Incentives::new(&*test_tube);
        let _ = incentives.create_gauge(
            MsgCreateGauge {
                is_perpetual: false,
                owner: chain.sender.address(),
                distribute_to: Some(QueryCondition {
                    lock_query_type: LockQueryType::NoLock.into(),
                    denom: "".to_owned(),
                    duration: None,
                    timestamp: None,
                }),
                coins: vec![v1beta1::Coin {
                    denom: GAS_DENOM.to_owned(),
                    amount: "100000000".to_owned(),
                }],
                start_time: Some(Timestamp {
                    seconds: time.seconds() as i64,
                    nanos: time.subsec_nanos() as i32,
                }),
                num_epochs_paid_over: 10,
                pool_id,
            },
            &chain.sender,
        )?;
    }
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

    // Check autocompound adds liquidity from the rewards, user balance remain unchanged
    // and rewards gets passed to the "stranger"

    // Check it has some rewards to autocompound first
    let pool_rewards = carrot_app.compound_status()?.pool_rewards;
    assert!(pool_rewards.iter().any(|c| c.denom == GAS_DENOM));

    // Save balances
    let balance_before_autocompound: AssetsBalanceResponse = carrot_app.balance()?;

    // Autocompound by stranger
    chain.wait_seconds(300)?;
    // Check query is able to compute rewards, when swap is required
    let compound_status: CompoundStatusResponse = carrot_app.compound_status()?;
    assert_eq!(compound_status.status, CompoundStatus::Ready {});
    assert_eq!(
        compound_status.autocompound_reward,
        AssetBase::native(REWARD_DENOM, 1000u128)
    );
    assert!(compound_status.autocompound_reward_available);
    carrot_app.call_as(&stranger).autocompound()?;

    // Save new balances
    let balance_after_autocompound: AssetsBalanceResponse = carrot_app.balance()?;

    // Liquidity added
    assert!(balance_after_autocompound.liquidity > balance_before_autocompound.liquidity);

    // Check it used all of the rewards
    let rewards = carrot_app.compound_status()?.pool_rewards;
    assert!(rewards.is_empty());

    // Check stranger gets rewarded
    let stranger_reward_balance = chain.query_balance(stranger.address().as_str(), REWARD_DENOM)?;
    assert_eq!(stranger_reward_balance, Uint128::new(1000));
    Ok(())
}
