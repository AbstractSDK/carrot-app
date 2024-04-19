mod common;

use crate::common::{setup_test_tube, USDC, USDT};
use carrot_app::msg::AppExecuteMsgFns;
use cosmwasm_std::{coin, coins};
use cw_orch::{anyhow, prelude::*};
use osmosis_std::types::osmosis::{
    gamm::v1beta1::{MsgSwapExactAmountIn, MsgSwapExactAmountInResponse},
    poolmanager::v1beta1::SwapAmountInRoute,
};
use prost_types::Any;

#[test]
fn deposit_after_inbalance_works() -> anyhow::Result<()> {
    let (pool_id, carrot_app) = setup_test_tube(false)?;

    // We should add funds to the account proxy
    let deposit_amount = 5_000;
    let deposit_coins = coins(deposit_amount, USDT.to_owned());
    let mut chain = carrot_app.get_chain().clone();
    let proxy = carrot_app.account().proxy()?;
    chain.add_balance(proxy.to_string(), deposit_coins.clone())?;

    // Do the deposit
    carrot_app.deposit(deposit_coins.clone(), None)?;

    // Create a pool inbalance by swapping a lot deposit amount from one to the other.
    // All the positions in the pool are centered, so the price doesn't change, just the funds ratio inside the position

    let swap_msg = MsgSwapExactAmountIn {
        sender: chain.sender().to_string(),
        token_in: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: USDT.to_string(),
            amount: "10_000".to_string(),
        }),
        token_out_min_amount: "1".to_string(),
        routes: vec![SwapAmountInRoute {
            pool_id,
            token_out_denom: USDC.to_string(),
        }],
    }
    .to_any();
    chain.commit_any::<MsgSwapExactAmountInResponse>(
        vec![Any {
            type_url: swap_msg.type_url,
            value: swap_msg.value,
        }],
        None,
    )?;

    let proxy_balance_before_second = chain
        .bank_querier()
        .balance(&proxy, Some(USDT.to_string()))?[0]
        .amount;
    // Add some more funds
    chain.add_balance(proxy.to_string(), deposit_coins.clone())?;

    // // Do the second deposit
    carrot_app.deposit(vec![coin(deposit_amount, USDT.to_owned())], None)?;
    // Check almost everything landed
    let proxy_balance_after_second = chain
        .bank_querier()
        .balance(&proxy, Some(USDT.to_string()))?[0]
        .amount;

    // Assert second deposit is more efficient than the first one
    assert!(proxy_balance_after_second - proxy_balance_before_second < proxy_balance_before_second);

    Ok(())
}
