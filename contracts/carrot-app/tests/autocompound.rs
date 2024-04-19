mod common;

use crate::common::{setup_test_tube, DEX_NAME, EXECUTOR_REWARD, GAS_DENOM, LOTS, USDC, USDT};
use abstract_app::abstract_interface::{Abstract, AbstractAccount};
use carrot_app::msg::{
    AppExecuteMsgFns, AppQueryMsgFns, AssetsBalanceResponse, AvailableRewardsResponse,
    CompoundStatus,
};
use cosmwasm_std::{coin, coins};
use cw_orch::osmosis_test_tube::osmosis_test_tube::Account;
use cw_orch::{anyhow, prelude::*};

#[test]
fn check_autocompound() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let mut chain = carrot_app.get_chain().clone();

    // Create position
    let deposit_amount = 5_000;
    let deposit_coins = coins(deposit_amount, USDT.to_owned());
    chain.add_balance(
        carrot_app.account().proxy()?.to_string(),
        deposit_coins.clone(),
    )?;

    // Do the deposit
    carrot_app.deposit(deposit_coins.clone(), None)?;

    // Do some swaps
    let dex: abstract_dex_adapter::interface::DexAdapter<_> = carrot_app.module()?;
    let abs = Abstract::load_from(chain.clone())?;
    let account_id = carrot_app.account().id()?;
    let account = AbstractAccount::new(&abs, account_id);
    chain.bank_send(
        account.proxy.addr_str()?,
        vec![
            coin(2_000_000, USDC.to_owned()),
            coin(2_000_000, USDT.to_owned()),
        ],
    )?;
    for _ in 0..10 {
        dex.ans_swap((USDC, 500_000), USDT, DEX_NAME.to_string(), &account)?;
        dex.ans_swap((USDT, 500_000), USDC, DEX_NAME.to_string(), &account)?;
    }

    // Check autocompound adds liquidity from the rewards and user balance remain unchanged

    // Check it has some rewards to autocompound first
    let rewards = carrot_app.available_rewards()?;
    assert!(!rewards.available_rewards.balances.is_empty());

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
    assert!(balance_after_autocompound.total_value > balance_before_autocompound.total_value);
    // Only rewards went in there
    assert!(balance_usdc_after_autocompound.amount >= balance_usdc_before_autocompound.amount);
    assert!(balance_usdt_after_autocompound.amount >= balance_usdt_before_autocompound.amount,);
    // Check it used all of the rewards
    let rewards: AvailableRewardsResponse = carrot_app.available_rewards()?;
    assert!(rewards.available_rewards.balances.is_empty());

    Ok(())
}

#[test]
fn stranger_autocompound() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let mut chain = carrot_app.get_chain().clone();
    let stranger = chain.init_account(coins(LOTS, GAS_DENOM))?;

    // Create position
    let deposit_amount = 5_000;
    let deposit_coins = coins(deposit_amount, USDT.to_owned());
    chain.add_balance(
        carrot_app.account().proxy()?.to_string(),
        deposit_coins.clone(),
    )?;

    // Do the deposit
    carrot_app.deposit(deposit_coins.clone(), None)?;

    // Do some swaps
    let dex: abstract_dex_adapter::interface::DexAdapter<_> = carrot_app.module()?;
    let abs = Abstract::load_from(chain.clone())?;
    let account_id = carrot_app.account().id()?;
    let account = AbstractAccount::new(&abs, account_id);
    chain.bank_send(
        account.proxy.addr_str()?,
        vec![
            coin(2_000_000, USDC.to_owned()),
            coin(2_000_000, USDT.to_owned()),
        ],
    )?;
    for _ in 0..10 {
        dex.ans_swap((USDC, 500_000), USDT, DEX_NAME.to_string(), &account)?;
        dex.ans_swap((USDT, 500_000), USDC, DEX_NAME.to_string(), &account)?;
    }

    // Check autocompound adds liquidity from the rewards, user balance remain unchanged
    // and rewards gets passed to the "stranger"

    // Check it has some rewards to autocompound first
    let available_rewards: AvailableRewardsResponse = carrot_app.available_rewards()?;
    assert!(!available_rewards.available_rewards.balances.is_empty());

    // Save balances
    let balance_before_autocompound: AssetsBalanceResponse = carrot_app.balance()?;

    // Autocompound by stranger
    chain.wait_seconds(300)?;
    // Check query is able to compute rewards, when swap is required
    let compound_status = carrot_app.compound_status()?;
    assert_eq!(compound_status.status, CompoundStatus::Ready {},);
    carrot_app.call_as(&stranger).autocompound()?;

    // Save new balances
    let balance_after_autocompound: AssetsBalanceResponse = carrot_app.balance()?;

    // Liquidity added
    assert!(balance_after_autocompound.total_value > balance_before_autocompound.total_value);

    // Check it used all of the rewards
    let rewards: AvailableRewardsResponse = carrot_app.available_rewards()?;
    assert!(rewards.available_rewards.balances.is_empty());

    // Check stranger gets rewarded

    for reward in available_rewards.available_rewards.balances {
        let stranger_reward_balance =
            chain.query_balance(stranger.address().as_str(), &reward.denom)?;
        assert_eq!(stranger_reward_balance, reward.amount * EXECUTOR_REWARD);
    }

    Ok(())
}
