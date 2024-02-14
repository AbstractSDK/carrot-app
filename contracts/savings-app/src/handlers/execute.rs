use super::swap_helpers::swap_to_enter_position;
use crate::{
    contract::{App, AppResult},
    helpers::get_user,
    msg::{AppExecuteMsg, CreatePositionMessage, ExecuteMsg},
    replies::{ADD_TO_POSITION_ID, CREATE_POSITION_ID},
    state::{assert_contract, get_osmosis_position, CONFIG, POSITION},
};
use abstract_app::abstract_sdk::features::AbstractResponse;
use abstract_app::abstract_sdk::AuthZInterface;
use abstract_app::AppError;
use cosmwasm_std::{
    to_json_binary, Coin, CosmosMsg, DepsMut, Env, MessageInfo, SubMsg, Uint128, WasmMsg,
};
use cosmwasm_std::{Coins, Deps};
use osmosis_std::{
    cosmwasm_to_proto_coins, try_proto_to_cosmwasm_coins,
    types::osmosis::concentratedliquidity::v1beta1::{
        MsgAddToPosition, MsgCollectIncentives, MsgCollectSpreadRewards, MsgCreatePosition,
        MsgWithdrawPosition,
    },
};
use std::str::FromStr;

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
    mut create_position_msg: CreatePositionMessage,
) -> AppResult {
    // TODO verify authz permissions before creating the position
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;
    let mut response = app.response("create_position");
    // We start by checking if there is already a position
    let funds = create_position_msg.funds;
    let funds_to_deposit = if POSITION.exists(deps.storage) {
        let (withdraw_msg, withdraw_amount, total_amount, withdrawn_funds) =
            _inner_withdraw(deps.as_ref(), &env, None, &app)?;

        response = response
            .add_message(withdraw_msg)
            .add_attribute("withdraw_amount", withdraw_amount)
            .add_attribute("total_amount", total_amount);

        // We add the withdrawn funds to the input funds
        let mut coins: Coins = funds.try_into()?;
        for fund in withdrawn_funds {
            coins.add(fund)?;
        }
        POSITION.remove(deps.storage);
        coins.to_vec()
    } else {
        funds
    };

    create_position_msg.funds = funds_to_deposit;
    let (swap_messages, create_position_msg) =
        _create_position(deps.as_ref(), &env, &app, create_position_msg)?;

    Ok(response
        .add_messages(swap_messages)
        .add_submessage(create_position_msg))
}

fn deposit(deps: DepsMut, env: Env, info: MessageInfo, funds: Vec<Coin>, app: App) -> AppResult {
    // Only the admin (manager contracts or account owner) + the smart contract can deposit
    app.admin
        .assert_admin(deps.as_ref(), &info.sender)
        .or(assert_contract(&info, &env))?;

    let pool = get_osmosis_position(deps.as_ref())?;
    let position = pool.position.unwrap();

    let asset0 = try_proto_to_cosmwasm_coins(pool.asset0.clone())?[0].clone();
    let asset1 = try_proto_to_cosmwasm_coins(pool.asset1.clone())?[0].clone();

    // When depositing, we start by adapting the available funds to the expected pool funds ratio
    // We do so by computing the swap information

    let (swap_msgs, resulting_assets) =
        swap_to_enter_position(deps.as_ref(), &env, funds, &app, asset0, asset1)?;

    let user = get_user(deps.as_ref(), &app)?;

    let deposit_msg = app.auth_z(deps.as_ref(), Some(user.clone()))?.execute(
        &env.contract.address,
        MsgAddToPosition {
            position_id: position.position_id,
            sender: user.to_string(),
            amount0: resulting_assets[0].amount.to_string(),
            amount1: resulting_assets[1].amount.to_string(),
            token_min_amount0: "0".to_string(), // No min, this always works
            token_min_amount1: "0".to_string(), // No min, this always works
        },
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

    let (withdraw_msg, withdraw_amount, total_amount, _withdrawn_funds) =
        _inner_withdraw(deps.as_ref(), &env, amount, &app)?;

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

    let position = get_osmosis_position(deps.as_ref())?;
    let position_details = position.position.unwrap();

    let mut rewards = cosmwasm_std::Coins::default();
    let mut collect_rewards_msgs = vec![];

    let user = get_user(deps.as_ref(), &app)?;
    let authz = app.auth_z(deps.as_ref(), Some(user.clone()))?;
    if !position.claimable_incentives.is_empty() {
        for coin in try_proto_to_cosmwasm_coins(position.claimable_incentives)? {
            rewards.add(coin)?;
        }
        collect_rewards_msgs.push(authz.execute(
            &env.contract.address,
            MsgCollectIncentives {
                position_ids: vec![position_details.position_id],
                sender: user.to_string(),
            },
        ));
    }

    if !position.claimable_spread_rewards.is_empty() {
        for coin in try_proto_to_cosmwasm_coins(position.claimable_spread_rewards)? {
            rewards.add(coin)?;
        }
        collect_rewards_msgs.push(authz.execute(
            &env.contract.address,
            MsgCollectSpreadRewards {
                position_ids: vec![position_details.position_id],
                sender: position_details.address.clone(),
            },
        ))
    }

    if rewards.is_empty() {
        return Err(crate::error::AppError::NoRewards {});
    }

    // Finally we ask for a deposit of all rewards from the wallet to the position
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

fn _inner_withdraw(
    deps: Deps,
    env: &Env,
    amount: Option<Uint128>,
    app: &App,
) -> AppResult<(CosmosMsg, String, String, Vec<Coin>)> {
    let position = get_osmosis_position(deps)?;
    let position_details = position.position.unwrap();

    let total_liquidity = position_details.liquidity.replace('.', "");

    let liquidity_amount = if let Some(amount) = amount {
        amount.to_string()
    } else {
        // TODO: it's decimals inside contracts
        total_liquidity.clone()
    };
    let user = get_user(deps, app)?;

    // We need to execute withdraw on the user's behalf
    let msg = app.auth_z(deps, Some(user.clone()))?.execute(
        &env.contract.address,
        MsgWithdrawPosition {
            position_id: position_details.position_id,
            sender: user.to_string(),
            liquidity_amount: liquidity_amount.clone(),
        },
    );

    let withdrawn_funds = vec![
        try_proto_to_cosmwasm_coins(position.asset0)?
            .first()
            .map(|c| {
                Ok::<_, AppError>(Coin {
                    denom: c.denom.clone(),
                    amount: c.amount * Uint128::from_str(&liquidity_amount)?
                        / Uint128::from_str(&total_liquidity)?,
                })
            })
            .transpose()?,
        try_proto_to_cosmwasm_coins(position.asset1)?
            .first()
            .map(|c| {
                Ok::<_, AppError>(Coin {
                    denom: c.denom.clone(),
                    amount: c.amount * Uint128::from_str(&liquidity_amount)?
                        / Uint128::from_str(&total_liquidity)?,
                })
            })
            .transpose()?,
    ]
    .into_iter()
    .flatten()
    .collect();

    Ok((msg, liquidity_amount, total_liquidity, withdrawn_funds))
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

    // With the current funds, we need to be able to create a position that makes sense
    // Therefore we swap the incoming funds to fit inside the future position
    let (swap_msgs, resulting_assets) =
        swap_to_enter_position(deps, &env, funds, &app, asset0, asset1)?;

    let sender = get_user(deps, &app)?;

    let create_msg = app.auth_z(deps, Some(sender.clone()))?.execute(
        &env.contract.address,
        MsgCreatePosition {
            pool_id: config.pool_config.pool_id,
            sender: sender.to_string(),
            lower_tick,
            upper_tick,
            tokens_provided: cosmwasm_to_proto_coins(resulting_assets),
            token_min_amount0: "0".to_string(), // No min amount here
            token_min_amount1: "0".to_string(), // No min amount, we want to deposit whatever we can
        },
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
