use abstract_app::{
    objects::{AnsAsset, AssetEntry},
    sdk::AuthZInterface,
};
use abstract_dex_adapter::{msg::GenerateMessagesResponse, DexInterface};
use cosmwasm_std::{Coin, CosmosMsg, Decimal, Deps, Env, Uint128};
use osmosis_std::cosmwasm_to_proto_coins;
pub const DEFAULT_MAX_SPREAD: Decimal = Decimal::percent(20);

use crate::{
    contract::{App, AppResult, OSMOSIS},
    helpers::get_user,
    state::CONFIG,
};

use super::query::query_price;

/// Assets to be included to the position
pub struct AssetsForPosition {
    pub asset0: Coin,
    pub asset1: Coin,
}

impl From<AssetsForPosition> for Vec<osmosis_std::types::cosmos::base::v1beta1::Coin> {
    fn from(value: AssetsForPosition) -> Self {
        let coins = match value.asset0.denom.cmp(&value.asset1.denom) {
            std::cmp::Ordering::Less => [value.asset0, value.asset1],
            _ => [value.asset1, value.asset0],
        };
        cosmwasm_to_proto_coins(coins)
    }
}

pub(crate) fn swap_msg(
    deps: Deps,
    env: &Env,
    offer_asset: AnsAsset,
    ask_asset: AssetEntry,
    max_spread: Option<Decimal>,
    app: &App,
) -> AppResult<Vec<CosmosMsg>> {
    // Don't swap if not required
    if offer_asset.amount.is_zero() {
        return Ok(vec![]);
    }
    let sender = get_user(deps, app)?;

    let dex = app.ans_dex(deps, env, OSMOSIS.to_string());
    let max_spread = Some(max_spread.unwrap_or(DEFAULT_MAX_SPREAD));
    let trigger_swap_msg: GenerateMessagesResponse =
        dex.generate_swap_messages(offer_asset, ask_asset, max_spread, None, sender.clone())?;
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
) -> AppResult<(AnsAsset, AssetEntry, AssetsForPosition)> {
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

    let (offer_asset, ask_asset, assets_for_position) = if x0_a1 < x1_a0 {
        let numerator = x1_a0 - x0_a1;
        let denominator = asset0.amount + asset1.amount.mul_floor(price);
        let y1 = numerator / denominator;

        (
            AnsAsset::new(config.pool_config.asset1, y1),
            config.pool_config.asset0,
            AssetsForPosition {
                asset0: Coin {
                    amount: x0.amount + y1.mul_floor(price),
                    denom: x0.denom,
                },
                asset1: Coin {
                    amount: x1.amount - y1,
                    denom: x1.denom,
                },
            },
        )
    } else {
        let numerator = x0_a1 - x1_a0;
        let denominator = asset1.amount
            + Uint128::one().mul_floor(Decimal::from_ratio(asset0.amount, 1u128) / price);
        let y0 = numerator / denominator;

        (
            AnsAsset::new(config.pool_config.asset0, numerator / denominator),
            config.pool_config.asset1,
            AssetsForPosition {
                asset0: Coin {
                    amount: x0.amount - y0,
                    denom: x0.denom,
                },
                asset1: Coin {
                    amount: x1.amount
                        + Uint128::one().mul_floor(Decimal::from_ratio(y0, 1u128) / price),
                    denom: x1.denom,
                },
            },
        )
    };

    Ok((offer_asset, ask_asset, assets_for_position))
}

#[allow(clippy::too_many_arguments)]
pub fn swap_to_enter_position(
    deps: Deps,
    env: &Env,
    funds: Vec<Coin>,
    app: &App,
    asset0: Coin,
    asset1: Coin,
    max_spread: Option<Decimal>,
    belief_price0: Option<Decimal>,
    belief_price1: Option<Decimal>,
) -> AppResult<(Vec<CosmosMsg>, AssetsForPosition)> {
    let price = query_price(
        deps,
        env,
        &funds,
        app,
        max_spread,
        belief_price0,
        belief_price1,
    )?;
    let (offer_asset, ask_asset, assets_for_position) =
        tokens_to_swap(deps, funds, asset0, asset1, price)?;

    Ok((
        swap_msg(deps, env, offer_asset, ask_asset, max_spread, app)?,
        assets_for_position,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::state::{AutocompoundRewardsConfig, Config, PoolConfig};
    use cosmwasm_std::{coin, coins, testing::mock_dependencies, DepsMut, Uint64};
    pub const DEPOSIT_TOKEN: &str = "USDC";
    pub const TOKEN0: &str = "USDT";
    pub const TOKEN1: &str = DEPOSIT_TOKEN;

    fn assert_is_around(result: Uint128, expected: impl Into<Uint128>) {
        let expected = expected.into().u128();
        let result = result.u128();

        if expected < result - 1 || expected > result + 1 {
            panic!("Results are not close enough")
        }
    }

    fn setup_config(deps: DepsMut) -> cw_orch::anyhow::Result<()> {
        CONFIG.save(
            deps.storage,
            &Config {
                pool_config: PoolConfig {
                    pool_id: 45,
                    asset0: AssetEntry::new(TOKEN0),
                    asset1: AssetEntry::new(TOKEN1),
                },
                autocompound_cooldown_seconds: Uint64::zero(),
                autocompound_rewards_config: AutocompoundRewardsConfig {
                    gas_asset: "foo".into(),
                    swap_asset: "bar".into(),
                    reward: Uint128::zero(),
                    min_gas_balance: Uint128::zero(),
                    max_gas_balance: Uint128::new(1),
                },
            },
        )?;
        Ok(())
    }

    // TODO: more tests on tokens_to_swap
    #[test]
    fn swap_for_ratio_one_to_one() {
        let mut deps = mock_dependencies();
        setup_config(deps.as_mut()).unwrap();
        let (swap, ask_asset, _final_asset) = tokens_to_swap(
            deps.as_ref(),
            coins(5_000, DEPOSIT_TOKEN),
            coin(100_000_000, TOKEN0),
            coin(100_000_000, TOKEN1),
            Decimal::one(),
        )
        .unwrap();

        assert_eq!(
            swap,
            AnsAsset {
                name: AssetEntry::new("usdc"),
                amount: Uint128::new(2500)
            }
        );
        assert_eq!(ask_asset, AssetEntry::new("usdt"));
    }

    #[test]
    fn swap_for_ratio_close_to_one() {
        let mut deps = mock_dependencies();
        setup_config(deps.as_mut()).unwrap();
        let amount0 = 110_000_000;
        let amount1 = 100_000_000;

        let (swap, ask_asset, _final_asset) = tokens_to_swap(
            deps.as_ref(),
            coins(5_000, DEPOSIT_TOKEN),
            coin(amount0, TOKEN0),
            coin(amount1, TOKEN1),
            Decimal::one(),
        )
        .unwrap();

        assert_is_around(swap.amount, 5_000 - 5_000 * amount1 / (amount1 + amount0));
        assert_eq!(swap.name, AssetEntry::new(TOKEN1));
        assert_eq!(ask_asset, AssetEntry::new(TOKEN0));
    }

    #[test]
    fn swap_for_ratio_far_from_one() {
        let mut deps = mock_dependencies();
        setup_config(deps.as_mut()).unwrap();
        let amount0 = 90_000_000;
        let amount1 = 10_000_000;
        let (swap, ask_asset, _final_asset) = tokens_to_swap(
            deps.as_ref(),
            coins(5_000, DEPOSIT_TOKEN),
            coin(amount0, TOKEN0),
            coin(amount1, TOKEN1),
            Decimal::one(),
        )
        .unwrap();

        assert_eq!(
            swap,
            AnsAsset {
                name: AssetEntry::new(DEPOSIT_TOKEN),
                amount: Uint128::new(5_000 - 5_000 * amount1 / (amount1 + amount0))
            }
        );
        assert_eq!(ask_asset, AssetEntry::new(TOKEN0));
    }

    #[test]
    fn swap_for_ratio_far_from_one_inverse() {
        let mut deps = mock_dependencies();
        setup_config(deps.as_mut()).unwrap();
        let amount0 = 10_000_000;
        let amount1 = 90_000_000;
        let (swap, ask_asset, _final_asset) = tokens_to_swap(
            deps.as_ref(),
            coins(5_000, DEPOSIT_TOKEN),
            coin(amount0, TOKEN0),
            coin(amount1, TOKEN1),
            Decimal::one(),
        )
        .unwrap();

        assert_is_around(swap.amount, 5_000 - 5_000 * amount1 / (amount1 + amount0));
        assert_eq!(swap.name, AssetEntry::new(TOKEN1));
        assert_eq!(ask_asset, AssetEntry::new(TOKEN0));
    }

    #[test]
    fn swap_for_non_unit_price() {
        let mut deps = mock_dependencies();
        setup_config(deps.as_mut()).unwrap();
        let amount0 = 10_000_000;
        let amount1 = 90_000_000;
        let price = Decimal::percent(150);
        let (swap, ask_asset, _final_asset) = tokens_to_swap(
            deps.as_ref(),
            coins(5_000, DEPOSIT_TOKEN),
            coin(amount0, TOKEN0),
            coin(amount1, TOKEN1),
            price,
        )
        .unwrap();

        assert_is_around(
            swap.amount,
            5_000
                - 5_000 * amount1
                    / (amount1
                        + (Uint128::one().mul_floor(Decimal::from_ratio(amount0, 1u128) / price))
                            .u128()),
        );
        assert_eq!(swap.name, AssetEntry::new(TOKEN1));
        assert_eq!(ask_asset, AssetEntry::new(TOKEN0));
    }

    #[test]
    fn swap_multiple_tokens_for_non_unit_price() {
        let mut deps = mock_dependencies();
        setup_config(deps.as_mut()).unwrap();
        let amount0 = 10_000_000;
        let amount1 = 10_000_000;
        let price = Decimal::percent(150);
        let (swap, ask_asset, _final_asset) = tokens_to_swap(
            deps.as_ref(),
            vec![coin(10_000, TOKEN0), coin(4_000, TOKEN1)],
            coin(amount0, TOKEN0),
            coin(amount1, TOKEN1),
            price,
        )
        .unwrap();

        assert_eq!(swap.name, AssetEntry::new(TOKEN0));
        assert_eq!(ask_asset, AssetEntry::new(TOKEN1));
        assert_eq!(
            10_000 - swap.amount.u128(),
            4_000
                + (Uint128::one().mul_floor(Decimal::from_ratio(swap.amount, 1u128) / price))
                    .u128()
        );
    }
}
