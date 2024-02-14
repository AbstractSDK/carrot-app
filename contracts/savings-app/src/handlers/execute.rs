use abstract_app::abstract_core::objects::{AnsAsset, AssetEntry};
use abstract_app::abstract_sdk::features::AbstractResponse;
use abstract_dex_adapter::{
    msg::{DexAction, DexExecuteMsg, DexQueryMsg, GenerateMessagesResponse},
    DexInterface,
};
use cosmwasm_std::{
    to_json_binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, SubMsg, Uint128,
    WasmMsg,
};
use osmosis_std::{
    cosmwasm_to_proto_coins, try_proto_to_cosmwasm_coins,
    types::osmosis::concentratedliquidity::v1beta1::{
        MsgAddToPosition, MsgCollectIncentives, MsgCollectSpreadRewards, MsgCreatePosition,
        MsgWithdrawPosition,
    },
};

use super::query::query_price;
use crate::msg::CreatePositionMessage;
use crate::{
    contract::{App, AppResult},
    helpers::{get_user, wrap_authz},
    msg::{AppExecuteMsg, ExecuteMsg},
    replies::{ADD_TO_POSITION_ID, CREATE_POSITION_ID},
    state::{assert_contract, get_osmosis_position, CONFIG, POSITION},
};
const MAX_SPREAD_PERCENT: u64 = 20;

pub fn execute_handler(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    app: App,
    msg: AppExecuteMsg,
) -> AppResult {
    match msg {
        AppExecuteMsg::CreatePosition(create_position_msg) => {
            create_position(deps, env, info, app, create_position_msg)
        }
        AppExecuteMsg::Deposit { funds } => deposit(deps, env, info, funds, app),
        AppExecuteMsg::Withdraw { amount } => withdraw(deps, env, info, Some(amount), app),
        AppExecuteMsg::WithdrawAll {} => withdraw(deps, env, info, None, app),
        AppExecuteMsg::Autocompound {} => autocompound(deps, env, info, app),
    }
}

fn create_position(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    app: App,
    create_position_msg: CreatePositionMessage,
) -> AppResult {
    // TODO verify authz permissions before creating the position
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;

    let (swap_msgs, create_msg) = _create_position(deps.as_ref(), &env, &app, create_position_msg)?;

    let mut response = app
        .response("create_position")
        .add_messages(swap_msgs)
        // We need to get the ID for this position in the reply
        .add_submessage(create_msg);

    // If we already have position open - withdraw all from it first
    if POSITION.exists(deps.storage) {
        let (withdraw_msg, withdraw_amount, total_amount) =
            _inner_withdraw(deps, &env, None, &app)?;
        response = response
            .add_message(withdraw_msg)
            .add_attribute("withdraw_amount", withdraw_amount)
            .add_attribute("total_amount", total_amount);
    }

    Ok(response)
}

fn deposit(deps: DepsMut, env: Env, info: MessageInfo, funds: Vec<Coin>, app: App) -> AppResult {
    // Only the authorized addresses (admin ?) + the smart contract can deposit
    app.admin
        .assert_admin(deps.as_ref(), &info.sender)
        .or(assert_contract(&info, &env))?;

    let pool = get_osmosis_position(deps.as_ref())?;
    let position = pool.position.unwrap();

    let asset0 = try_proto_to_cosmwasm_coins(pool.asset0.clone())?[0].clone();
    let asset1 = try_proto_to_cosmwasm_coins(pool.asset1.clone())?[0].clone();

    // When depositing, we start by adapting the available funds to the expected pool funds ratio
    // We do so by computing the swap information
    let price = query_price(deps.as_ref(), &funds, &app)?;

    let (offer_asset, ask_asset, resulting_assets) =
        tokens_to_swap(deps.as_ref(), funds, asset0, asset1, price)?;
    // Then we execute the swap
    let swap_msgs = swap_msg(deps.as_ref(), &env, offer_asset, ask_asset, &app)?;

    let deposit_msg = wrap_authz(
        MsgAddToPosition {
            position_id: position.position_id,
            sender: position.address.clone(),
            amount0: resulting_assets[0].amount.to_string(),
            amount1: resulting_assets[1].amount.to_string(),
            token_min_amount0: "0".to_string(), // No min, this always works
            token_min_amount1: "0".to_string(), // No min, this always works
        },
        position.address,
        &env,
    );

    Ok(app
        .response("deposit")
        .add_messages(swap_msgs)
        .add_submessage(SubMsg::reply_on_success(deposit_msg, ADD_TO_POSITION_ID)))
}

fn withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Option<Uint128>,
    app: App,
) -> AppResult {
    // Only the authorized addresses (admin ?) can withdraw
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;

    let (withdraw_msg, withdraw_amount, total_amount) = _inner_withdraw(deps, &env, amount, &app)?;

    Ok(app
        .response("withdraw")
        .add_attribute("withdraw_amount", withdraw_amount)
        .add_attribute("total_amount", total_amount)
        .add_message(withdraw_msg))
}

fn autocompound(deps: DepsMut, env: Env, _info: MessageInfo, app: App) -> AppResult {
    // TODO: shouldn't we have some limit either:
    // - config.cooldown
    // - min rewards to autocompound
    // Everyone can autocompound

    let pool = get_osmosis_position(deps.as_ref())?;
    let position = pool.position.unwrap();

    let mut rewards = cosmwasm_std::Coins::default();
    let mut collect_rewards_msgs = vec![];

    if !pool.claimable_incentives.is_empty() {
        for coin in try_proto_to_cosmwasm_coins(pool.claimable_incentives)? {
            rewards.add(coin)?;
        }
        collect_rewards_msgs.push(wrap_authz(
            MsgCollectIncentives {
                position_ids: vec![position.position_id],
                sender: position.address.clone(),
            },
            position.address.clone(),
            &env,
        ));
    }

    if !pool.claimable_spread_rewards.is_empty() {
        for coin in try_proto_to_cosmwasm_coins(pool.claimable_spread_rewards)? {
            rewards.add(coin)?;
        }
        collect_rewards_msgs.push(wrap_authz(
            MsgCollectSpreadRewards {
                position_ids: vec![position.position_id],
                sender: position.address.clone(),
            },
            position.address.clone(),
            &env,
        ))
    }

    if rewards.is_empty() {
        return Err(crate::error::AppError::NoRewards {});
    }

    // Finally we ask for a full deposit from the wallet to the position
    let msg_deposit = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::Deposit {
            funds: rewards.into(),
        }))?,
        funds: vec![],
    });

    Ok(app
        .response("auto-compound")
        .add_messages(collect_rewards_msgs)
        .add_message(msg_deposit))
}

fn swap_msg(
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
    let user = get_user(deps, app)?;

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
        addr_as_sender: user.clone(),
    };
    let trigger_swap_msg: GenerateMessagesResponse =
        dex.query(query_msg.clone()).map_err(|_| {
            cosmwasm_std::StdError::generic_err(format!(
                "Failed to query generate message, query_msg: {query_msg:?}"
            ))
        })?;

    Ok(trigger_swap_msg
        .messages
        .into_iter()
        .map(|m| wrap_authz(m, user.clone(), env))
        .collect())
}

fn tokens_to_swap(
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

fn _inner_withdraw(
    deps: DepsMut,
    env: &Env,
    amount: Option<Uint128>,
    _app: &App,
) -> AppResult<(CosmosMsg, String, String)> {
    let position = get_osmosis_position(deps.as_ref())?.position.unwrap();

    let liquidity_amount = if let Some(amount) = amount {
        amount.to_string()
    } else {
        // TODO: it's decimals inside contracts
        position.liquidity.replace('.', "")
    };

    // We need to execute withdraw on the user's behalf
    let msg = wrap_authz(
        MsgWithdrawPosition {
            position_id: position.position_id,
            sender: position.address.clone(),
            liquidity_amount: liquidity_amount.clone(),
        },
        position.address,
        env,
    );

    Ok((msg, liquidity_amount, position.liquidity))
}

pub(crate) fn _create_position(
    deps: Deps,
    env: &Env,
    app: &App,
    create_position_msg: CreatePositionMessage,
) -> AppResult<(Vec<CosmosMsg>, SubMsg)> {
    let config = CONFIG.load(deps.storage)?;

    let CreatePositionMessage {
        lower_tick,
        upper_tick,
        funds,
        asset0,
        asset1,
    } = create_position_msg;

    // First we do the swap
    let price = query_price(deps, &funds, app)?;
    let (offer_asset, ask_asset, resulting_assets) =
        tokens_to_swap(deps, funds, asset0, asset1, price)?;

    let swap_msgs = swap_msg(deps, env, offer_asset, ask_asset, app)?;

    let sender = get_user(deps, app)?;

    // Then we create the position
    let create_msg = wrap_authz(
        MsgCreatePosition {
            pool_id: config.pool_config.pool_id,
            sender: sender.clone(),
            lower_tick,
            upper_tick,
            tokens_provided: cosmwasm_to_proto_coins(resulting_assets),
            token_min_amount0: "0".to_string(), // No min amount here
            token_min_amount1: "0".to_string(), // No min amount, we want to deposit whatever we can
        },
        sender,
        env,
    );

    Ok((
        swap_msgs,
        SubMsg::reply_always(create_msg, CREATE_POSITION_ID),
    ))
}

#[cfg(test)]
mod tests {
    use cosmwasm_std::{coin, coins, testing::mock_dependencies};
    use cw_asset::AssetInfo;

    use super::*;
    use crate::state::{Config, PoolConfig};
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
                deposit_info: AssetInfo::Native(DEPOSIT_TOKEN.to_string()),
                pool_config: PoolConfig {
                    pool_id: 45,
                    token0: TOKEN0.to_string(),
                    token1: TOKEN1.to_string(),
                    asset0: AssetEntry::new(TOKEN0),
                    asset1: AssetEntry::new(TOKEN1),
                },
                exchange: "osmosis".to_string(),
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
                        + (Decimal::from_ratio(amount0, 1u128) / price * Uint128::one()).u128()),
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
            4_000 + (Decimal::from_ratio(swap.amount, 1u128) / price * Uint128::one()).u128()
        );
    }
}
