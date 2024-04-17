mod common;

use crate::common::{deposit_with_funds, setup_test_tube, DEX_NAME, USDC, USDT};
use abstract_app::{
    abstract_interface::{Abstract, AbstractAccount},
    objects::AnsAsset,
};
use carrot_app::msg::{
    AppExecuteMsgFns, AppQueryMsgFns, AssetsBalanceResponse, AvailableRewardsResponse,
};
use cosmwasm_std::coin;
use cw_orch::{anyhow, prelude::*};

#[test]
fn check_autocompound() -> anyhow::Result<()> {
    let (_, carrot_app) = setup_test_tube(false)?;

    let chain = carrot_app.get_chain().clone();

    // Create position
    let deposit_amount = 5_000u128;
    // Do the deposit
    deposit_with_funds(&carrot_app, vec![AnsAsset::new(USDT, deposit_amount)])?;

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
    let rewards: AvailableRewardsResponse = carrot_app.available_rewards()?;
    assert!(!rewards.available_rewards.is_empty());

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
    assert!(rewards.available_rewards.is_empty());

    Ok(())
}

// #[test]
// fn stranger_autocompound() -> anyhow::Result<()> {
//     let (_, carrot_app) = setup_test_tube(false)?;

//     let mut chain = carrot_app.get_chain().clone();
//     let stranger = chain.init_account(coins(LOTS, GAS_DENOM))?;

//     // Create position
//     create_position(
//         &carrot_app,
//         coins(100_000, USDT.to_owned()),
//         coin(1_000_000, USDT.to_owned()),
//         coin(1_000_000, USDC.to_owned()),
//     )?;

//     // Do some swaps
//     let dex: abstract_dex_adapter::interface::DexAdapter<_> = carrot_app.module()?;
//     let abs = Abstract::load_from(chain.clone())?;
//     let account_id = carrot_app.account().id()?;
//     let account = AbstractAccount::new(&abs, account_id);
//     chain.bank_send(
//         account.proxy.addr_str()?,
//         vec![
//             coin(200_000, USDC.to_owned()),
//             coin(200_000, USDT.to_owned()),
//         ],
//     )?;
//     for _ in 0..10 {
//         dex.ans_swap((USDC, 50_000), USDT, DEX_NAME.to_string(), &account)?;
//         dex.ans_swap((USDT, 50_000), USDC, DEX_NAME.to_string(), &account)?;
//     }

//     // Check autocompound adds liquidity from the rewards, user balance remain unchanged
//     // and rewards gets passed to the "stranger"

//     // Check it has some rewards to autocompound first
//     let rewards: AvailableRewardsResponse = carrot_app.available_rewards()?;
//     assert!(!rewards.available_rewards.is_empty());

//     // Save balances
//     let balance_before_autocompound: AssetsBalanceResponse = carrot_app.balance()?;

//     // Autocompound by stranger
//     chain.wait_seconds(300)?;
//     // Check query is able to compute rewards, when swap is required
//     let compound_status: CompoundStatusResponse = carrot_app.compound_status()?;
//     assert_eq!(
//         compound_status,
//         CompoundStatusResponse {
//             status: CompoundStatus::Ready {},
//             reward: AssetBase::native(REWARD_DENOM, 1000u128),
//             rewards_available: true
//         }
//     );
//     carrot_app.call_as(&stranger).autocompound()?;

//     // Save new balances
//     let balance_after_autocompound: AssetsBalanceResponse = carrot_app.balance()?;

//     // Liquidity added
//     assert!(balance_after_autocompound.liquidity > balance_before_autocompound.liquidity);

//     // Check it used all of the rewards
//     let rewards: AvailableRewardsResponse = carrot_app.available_rewards()?;
//     assert!(rewards.available_rewards.is_empty());

//     // Check stranger gets rewarded
//     let stranger_reward_balance = chain.query_balance(stranger.address().as_str(), REWARD_DENOM)?;
//     assert_eq!(stranger_reward_balance, Uint128::new(1000));
//     Ok(())
// }
