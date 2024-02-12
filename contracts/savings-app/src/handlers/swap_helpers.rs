use abstract_app::objects::{AnsAsset, AssetEntry};
use abstract_dex_adapter::{
    msg::{DexAction, DexExecuteMsg, DexQueryMsg, GenerateMessagesResponse},
    DexInterface,
};
use abstract_sdk::AuthZInterface;
use cosmwasm_std::{Coin, CosmosMsg, Decimal, Deps, Env, Uint128};
const MAX_SPREAD_PERCENT: u64 = 20;

use crate::{
    contract::{App, AppResult},
    helpers::get_user,
    state::CONFIG,
};

use super::query::query_price;

pub(crate) fn swap_msg(
    deps: Deps,
    env: &Env,
    offer_asset: AnsAsset,
    ask_asset: AssetEntry,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
    // Don't swap if not required
    if offer_asset.amount.is_zero() {
        return Ok(vec![]);
    }
    let config = CONFIG.load(deps.storage)?;
    let sender = get_user(deps, app)?;

    let dex = app.dex(deps, config.exchange.clone());
    let query_msg = DexQueryMsg::GenerateMessages {
        message: DexExecuteMsg::Action {
            dex: config.exchange,
            action: DexAction::Swap {
                offer_asset,
                ask_asset,
                max_spread: Some(Decimal::percent(MAX_SPREAD_PERCENT)),
                belief_price: None,
            },
        },
        addr_as_sender: sender.to_string(),
    };
    let trigger_swap_msg: GenerateMessagesResponse =
        dex.query(query_msg.clone()).map_err(|_| {
            cosmwasm_std::StdError::generic_err(format!(
                "Failed to query generate message, query_msg: {query_msg:?}"
            ))
        })?;
    let authz = app.auth_z(deps, Some(sender))?;

    Ok(trigger_swap_msg
        .messages
        .into_iter()
        .map(|m| authz.execute(&env.contract.address, m))
        .collect())
}

pub(crate) fn tokens_to_swap(
    deps: Deps,
    amount_to_swap: Vec<Coin>,
    asset0: Coin, // Represents the amount of Coin 0 we would like the position to handle
    asset1: Coin, // Represents the amount of Coin 1 we would like the position to handle,
    price: Decimal, // Relative price (when swapping amount0 for amount1, equals amount0/amount1)
) -> AppResult<(AnsAsset, AssetEntry, Vec<Coin>)> {
    let config = CONFIG.load(deps.storage)?;

    let x0 = amount_to_swap
        .iter()
        .find(|c| c.denom == asset0.denom)
        .cloned()
        .unwrap_or(Coin {
            denom: asset0.denom,
            amount: Uint128::zero(),
        });
    let x1 = amount_to_swap
        .iter()
        .find(|c| c.denom == asset1.denom)
        .cloned()
        .unwrap_or(Coin {
            denom: asset1.denom,
            amount: Uint128::zero(),
        });

    // We will swap on the pool to get the right coin ratio

    // We have x0 and x1 to deposit. Let p (or price) be the price of asset1 (the number of asset0 you get for 1 unit of asset1)
    // In order to deposit, you need to have X0 and X1 such that X0/X1 = A0/A1 where A0 and A1 are the current liquidity inside the position
    // That is equivalent to X0*A1 = X1*A0
    // We need to find how much to swap.
    // If x0*A1 < x1*A0, we need to have more x0 to balance the swap --> so we need to send some of x1 to swap (lets say we wend y1 to swap)
    // So   X1 = x1-y1
    //      X0 = x0 + price*y1
    // Therefore, the following equation needs to be true
    // (x0 + price*y1)*A1 = (x1-y1)*A0 or y1 = (x1*a0 - x0*a1)/(a0 + p*a1)
    // If x0*A1 > x1*A0, we need to have more x1 to balance the swap --> so we need to send some of x0 to swap (lets say we wend y0 to swap)
    // So   X0 = x0-y0
    //      X1 = x1 + y0/price
    // Therefore, the following equation needs to be true
    // (x0-y0)*A1 = (x1 + y0/price)*A0 or y0 = (x0*a1 - x1*a0)/(a1 + a0/p)

    let x0_a1 = x0.amount * asset1.amount;
    let x1_a0 = x1.amount * asset0.amount;

    // TODO: resulting_balance denoms is unsorted right now
    let (offer_asset, ask_asset, resulting_balance) = if x0_a1 < x1_a0 {
        let numerator = x1_a0 - x0_a1;
        let denominator = asset0.amount + price * asset1.amount;
        let y1 = numerator / denominator;

        (
            AnsAsset::new(config.pool_config.asset1, y1),
            config.pool_config.asset0,
            vec![
                Coin {
                    amount: x0.amount + price * y1,
                    denom: x0.denom,
                },
                Coin {
                    amount: x1.amount - y1,
                    denom: x1.denom,
                },
            ],
        )
    } else {
        let numerator = x0_a1 - x1_a0;
        let denominator =
            asset1.amount + Decimal::from_ratio(asset0.amount, 1u128) / price * Uint128::one();
        let y0 = numerator / denominator;

        (
            AnsAsset::new(config.pool_config.asset0, numerator / denominator),
            config.pool_config.asset1,
            vec![
                Coin {
                    amount: x0.amount - y0,
                    denom: x0.denom,
                },
                Coin {
                    amount: x1.amount + Decimal::from_ratio(y0, 1u128) / price * Uint128::one(),
                    denom: x1.denom,
                },
            ],
        )
    };

    // TODO, compute the resulting balance to be able to deposit back into the pool
    Ok((offer_asset, ask_asset, resulting_balance))
}

pub fn swap_to_enter_position(
    deps: Deps,
    env: &Env,
    funds: Vec<Coin>,
    app: &App,
    asset0: Coin,
    asset1: Coin,
) -> AppResult<(Vec<CosmosMsg>, Vec<Coin>)> {
    let price = query_price(deps, &funds, app)?;
    let (offer_asset, ask_asset, resulting_assets) =
        tokens_to_swap(deps, funds, asset0, asset1, price)?;

    Ok((
        swap_msg(deps, env, offer_asset, ask_asset, app)?,
        resulting_assets,
    ))
}
